//! Bounded Hyper HTTP/1 listener ownership for the native backend processes.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::future::{Future, poll_fn};
use std::io;
use std::net::SocketAddr;
use std::task::Poll;
use std::time::Duration;

use hyper::body::{Body, Incoming};
use hyper::rt::{Read, Write};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::{JoinError, JoinSet};
use tokio::time::{sleep, timeout};

use crate::{NativeHttpConnectionScope, NativeTrackedHttpBody};

const MINIMUM_HYPER_READ_BUFFER_BYTES: usize = 8_192;

type BoxError = Box<dyn StdError + Send + Sync>;

/// Explicit listener limits shared by the API, edge, and worker processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeHttpListenerConfiguration {
    graceful_shutdown_timeout: Duration,
    header_read_timeout: Duration,
    maximum_active_connections: usize,
    maximum_connection_lifetime: Duration,
    maximum_read_buffer_bytes: usize,
    vectored_writes: bool,
}

impl NativeHttpListenerConfiguration {
    /// Validate every resource and deadline before a socket can be accepted.
    pub fn new(
        maximum_active_connections: usize,
        header_read_timeout: Duration,
        maximum_connection_lifetime: Duration,
        graceful_shutdown_timeout: Duration,
        maximum_read_buffer_bytes: usize,
        vectored_writes: bool,
    ) -> Result<Self, NativeHttpListenerConfigurationError> {
        // reject an unbounded-by-construction listener before entering its accept loop
        if maximum_active_connections == 0 {
            return Err(NativeHttpListenerConfigurationError::NoConnectionCapacity);
        }
        // keep Hyper's timer-backed header deadline effective
        if header_read_timeout.is_zero() {
            return Err(NativeHttpListenerConfigurationError::ZeroHeaderReadTimeout);
        }
        // prevent permanent keep-alive ownership from consuming every slot
        if maximum_connection_lifetime.is_zero() {
            return Err(NativeHttpListenerConfigurationError::ZeroConnectionLifetime);
        }
        // give in-flight work a finite drain window after admission closes
        if graceful_shutdown_timeout.is_zero() {
            return Err(NativeHttpListenerConfigurationError::ZeroGracefulShutdownTimeout);
        }
        // Hyper requires at least 8 KiB for its HTTP/1 read buffer
        if maximum_read_buffer_bytes < MINIMUM_HYPER_READ_BUFFER_BYTES {
            return Err(NativeHttpListenerConfigurationError::ReadBufferTooSmall {
                actual: maximum_read_buffer_bytes,
                minimum: MINIMUM_HYPER_READ_BUFFER_BYTES,
            });
        }
        Ok(Self {
            graceful_shutdown_timeout,
            header_read_timeout,
            maximum_active_connections,
            maximum_connection_lifetime,
            maximum_read_buffer_bytes,
            vectored_writes,
        })
    }

    /// Return the maximum number of accepted sockets retained at once.
    pub const fn maximum_active_connections(self) -> usize {
        self.maximum_active_connections
    }
}

/// Invalid limits that would weaken the listener's bounded-resource contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeHttpListenerConfigurationError {
    NoConnectionCapacity,
    ReadBufferTooSmall { actual: usize, minimum: usize },
    ZeroConnectionLifetime,
    ZeroGracefulShutdownTimeout,
    ZeroHeaderReadTimeout,
}

impl Display for NativeHttpListenerConfigurationError {
    /// Describe the rejected limit without exposing process-specific configuration.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoConnectionCapacity => formatter.write_str("HTTP connection capacity is zero"),
            Self::ReadBufferTooSmall { actual, minimum } => write!(
                formatter,
                "HTTP read buffer is {actual} bytes; Hyper requires at least {minimum} bytes"
            ),
            Self::ZeroConnectionLifetime => formatter.write_str("HTTP connection lifetime is zero"),
            Self::ZeroGracefulShutdownTimeout => {
                formatter.write_str("HTTP graceful shutdown timeout is zero")
            }
            Self::ZeroHeaderReadTimeout => formatter.write_str("HTTP header read timeout is zero"),
        }
    }
}

impl StdError for NativeHttpListenerConfigurationError {}

/// Aggregate transport outcomes retained without turning peer failures into process failures.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NativeHttpListenerReport {
    pub accepted_connections: usize,
    pub clean_connections: usize,
    pub connection_lifetime_expirations: usize,
    pub forced_shutdown_connections: usize,
    pub graceful_shutdown_expirations: usize,
    pub protocol_errors: usize,
}

/// Listener-level failures that prevent reliable admission or task ownership.
#[derive(Debug)]
pub enum NativeHttpListenerError {
    Accept(io::Error),
    ConnectionTask(JoinError),
}

impl Display for NativeHttpListenerError {
    /// Identify which listener ownership boundary failed.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accept(error) => write!(formatter, "HTTP listener accept failed: {error}"),
            Self::ConnectionTask(error) => {
                write!(formatter, "HTTP connection task failed: {error}")
            }
        }
    }
}

impl StdError for NativeHttpListenerError {
    /// Preserve the operating-system or task failure as the error source.
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Accept(error) => Some(error),
            Self::ConnectionTask(error) => Some(error),
        }
    }
}

enum ListenerActivity {
    Accepted(io::Result<(TcpStream, SocketAddr)>),
    ConnectionFinished(Result<NativeHttpConnectionOutcome, JoinError>),
    Shutdown,
}

enum NativeHttpConnectionOutcome {
    Clean,
    ConnectionLifetimeExpired,
    GracefulShutdownExpired,
    ProtocolError,
}

enum NativeHttpConnectionTrigger {
    Completed(Result<(), hyper::Error>),
    ConnectionLifetimeExpired,
    Shutdown,
}

/// Serve bounded HTTP/1 connections until the supplied process shutdown future resolves.
///
/// The caller must run this future inside a Tokio `LocalSet`. This keeps all listener and
/// connection work compatible with a current-thread runtime while still permitting local,
/// non-`Send` application state. Extra peers remain in the kernel listen queue until one of
/// the explicitly retained connection tasks is reaped.
pub async fn serve_native_http1<Handler, HandlerFuture, ResponseBody, HandlerError, Shutdown>(
    listener: TcpListener,
    configuration: NativeHttpListenerConfiguration,
    handler: Handler,
    shutdown: Shutdown,
) -> Result<NativeHttpListenerReport, NativeHttpListenerError>
where
    Handler: Fn(Request<Incoming>, SocketAddr) -> HandlerFuture + Clone + 'static,
    HandlerFuture: Future<Output = Result<Response<ResponseBody>, HandlerError>> + 'static,
    ResponseBody: Body + 'static,
    ResponseBody::Error: Into<BoxError>,
    HandlerError: Into<BoxError> + 'static,
    Shutdown: Future<Output = ()>,
{
    serve_native_http1_connections(
        listener,
        configuration,
        move |socket, peer, configuration, stop| {
            let handler = handler.clone();
            serve_native_http1_connection(
                TokioIo::new(socket),
                configuration,
                move |request| handler(request, peer),
                stop,
            )
        },
        shutdown,
    )
    .await
}

/// Serve the public edge with one real response-terminal owner per admitted connection.
pub async fn serve_native_edge_http1<Handler, HandlerFuture, HandlerError, Shutdown>(
    listener: TcpListener,
    configuration: NativeHttpListenerConfiguration,
    handler: Handler,
    shutdown: Shutdown,
) -> Result<NativeHttpListenerReport, NativeHttpListenerError>
where
    Handler: Fn(Request<Incoming>, SocketAddr, NativeHttpConnectionScope) -> HandlerFuture
        + Clone
        + 'static,
    HandlerFuture: Future<Output = Result<Response<NativeTrackedHttpBody>, HandlerError>> + 'static,
    HandlerError: Into<BoxError> + 'static,
    Shutdown: Future<Output = ()>,
{
    serve_native_http1_connections(
        listener,
        configuration,
        move |socket, peer, configuration, stop| {
            let scope = NativeHttpConnectionScope::new();
            let io = scope.tracked_io(TokioIo::new(socket));
            let handler = handler.clone();
            serve_native_http1_connection(
                io,
                configuration,
                move |request| {
                    let scope = scope.clone();
                    let handler = handler.clone();
                    async move {
                        // route a subsequent request only after the preceding actual terminal
                        scope.wait_until_idle().await.map_err(BoxError::from)?;
                        handler(request, peer, scope).await.map_err(Into::into)
                    }
                },
                stop,
            )
        },
        shutdown,
    )
    .await
}

/// Share connection bounds and shutdown between plain and forecast-tracked listeners.
async fn serve_native_http1_connections<Connection, ConnectionFuture, Shutdown>(
    listener: TcpListener,
    configuration: NativeHttpListenerConfiguration,
    connection: Connection,
    shutdown: Shutdown,
) -> Result<NativeHttpListenerReport, NativeHttpListenerError>
where
    Connection: Fn(
        TcpStream,
        SocketAddr,
        NativeHttpListenerConfiguration,
        watch::Receiver<bool>,
    ) -> ConnectionFuture,
    ConnectionFuture: Future<Output = NativeHttpConnectionOutcome> + 'static,
    Shutdown: Future<Output = ()>,
{
    let (stop_sender, stop_receiver) = watch::channel(false);
    let mut connections = JoinSet::new();
    let mut report = NativeHttpListenerReport::default();
    let mut shutdown = Box::pin(shutdown);
    // admit only while process shutdown is pending
    loop {
        let activity = poll_fn(|context| {
            // release completed tasks before considering another accept
            if !connections.is_empty()
                && let Poll::Ready(Some(outcome)) = connections.poll_join_next(context)
            {
                return Poll::Ready(ListenerActivity::ConnectionFinished(outcome));
            }
            // stop admission before polling the listener again
            if shutdown.as_mut().poll(context).is_ready() {
                return Poll::Ready(ListenerActivity::Shutdown);
            }
            // leave excess peers in the kernel backlog rather than spawning socket waiters
            if connections.len() < configuration.maximum_active_connections {
                return listener
                    .poll_accept(context)
                    .map(ListenerActivity::Accepted);
            }
            Poll::Pending
        })
        .await;
        match activity {
            ListenerActivity::Accepted(result) => {
                let (socket, peer_address) = result.map_err(NativeHttpListenerError::Accept)?;
                report.accepted_connections += 1;
                let connection_stop = stop_receiver.clone();
                let _task = connections.spawn_local(connection(
                    socket,
                    peer_address,
                    configuration,
                    connection_stop,
                ));
            }
            ListenerActivity::ConnectionFinished(result) => {
                record_connection_outcome(
                    result.map_err(NativeHttpListenerError::ConnectionTask)?,
                    &mut report,
                );
            }
            ListenerActivity::Shutdown => break,
        }
    }
    // close the listening socket before asking retained connections to drain
    drop(listener);
    // wake every retained connection without allocating one signal per task
    let _receivers_already_closed = stop_sender.send(true);
    let graceful_timeout = configuration.graceful_shutdown_timeout;
    let drain = async {
        // reap each retained task before the process can drop its transport state
        while let Some(result) = connections.join_next().await {
            record_connection_outcome(
                result.map_err(NativeHttpListenerError::ConnectionTask)?,
                &mut report,
            );
        }
        Ok::<(), NativeHttpListenerError>(())
    };
    match timeout(graceful_timeout, drain).await {
        Ok(result) => result?,
        Err(_) => {
            report.forced_shutdown_connections = connections.len();
            connections.abort_all();
            // observe every cancellation so no detached task survives the listener future
            while connections.join_next().await.is_some() {}
        }
    }
    Ok(report)
}

/// Drive one accepted Hyper connection through its lifetime and graceful-stop limits.
async fn serve_native_http1_connection<IO, Handler, HandlerFuture, ResponseBody, HandlerError>(
    io: IO,
    configuration: NativeHttpListenerConfiguration,
    handler: Handler,
    mut stop: watch::Receiver<bool>,
) -> NativeHttpConnectionOutcome
where
    IO: Read + Write + Unpin + 'static,
    Handler: Fn(Request<Incoming>) -> HandlerFuture + 'static,
    HandlerFuture: Future<Output = Result<Response<ResponseBody>, HandlerError>> + 'static,
    ResponseBody: Body + 'static,
    ResponseBody::Error: Into<BoxError>,
    HandlerError: Into<BoxError> + 'static,
{
    let service = service_fn(handler);
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(configuration.header_read_timeout)
        .pipeline_flush(false)
        .max_buf_size(configuration.maximum_read_buffer_bytes)
        .keep_alive(true)
        .writev(configuration.vectored_writes);
    let mut connection = Box::pin(builder.serve_connection(io, service));
    let mut lifetime = Box::pin(sleep(configuration.maximum_connection_lifetime));
    let mut stop_changed = Box::pin(stop.changed());
    let trigger = poll_fn(|context| {
        // prevent Hyper from routing another buffered request once stop is observable
        if stop_changed.as_mut().poll(context).is_ready() {
            return Poll::Ready(NativeHttpConnectionTrigger::Shutdown);
        }
        // prefer a transport terminal result when it coincides with another deadline
        if let Poll::Ready(result) = connection.as_mut().poll(context) {
            return Poll::Ready(NativeHttpConnectionTrigger::Completed(result));
        }
        // expire old keep-alive owners independently of header progress
        if lifetime.as_mut().poll(context).is_ready() {
            return Poll::Ready(NativeHttpConnectionTrigger::ConnectionLifetimeExpired);
        }
        Poll::Pending
    })
    .await;
    match trigger {
        NativeHttpConnectionTrigger::Completed(Ok(())) => NativeHttpConnectionOutcome::Clean,
        NativeHttpConnectionTrigger::Completed(Err(_)) => {
            NativeHttpConnectionOutcome::ProtocolError
        }
        NativeHttpConnectionTrigger::ConnectionLifetimeExpired => {
            connection.as_mut().graceful_shutdown();
            match timeout(configuration.graceful_shutdown_timeout, connection).await {
                Ok(Ok(())) => NativeHttpConnectionOutcome::ConnectionLifetimeExpired,
                Ok(Err(_)) => NativeHttpConnectionOutcome::ProtocolError,
                Err(_) => NativeHttpConnectionOutcome::GracefulShutdownExpired,
            }
        }
        NativeHttpConnectionTrigger::Shutdown => {
            connection.as_mut().graceful_shutdown();
            // the listener's single outer deadline owns the process-wide drain budget
            match connection.await {
                Ok(()) => NativeHttpConnectionOutcome::Clean,
                Err(_) => NativeHttpConnectionOutcome::ProtocolError,
            }
        }
    }
}

/// Add one reaped connection to the bounded listener's aggregate receipt.
fn record_connection_outcome(
    outcome: NativeHttpConnectionOutcome,
    report: &mut NativeHttpListenerReport,
) {
    match outcome {
        NativeHttpConnectionOutcome::Clean => report.clean_connections += 1,
        NativeHttpConnectionOutcome::ConnectionLifetimeExpired => {
            report.connection_lifetime_expirations += 1;
        }
        NativeHttpConnectionOutcome::GracefulShutdownExpired => {
            report.graceful_shutdown_expirations += 1;
        }
        NativeHttpConnectionOutcome::ProtocolError => report.protocol_errors += 1,
    }
}
