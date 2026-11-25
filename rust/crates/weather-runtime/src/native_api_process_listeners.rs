//! Concrete socket and application ownership for the native API process.

use std::cell::RefCell;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::future::{Future, poll_fn};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::num::NonZeroU16;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;

use hyper::body::Incoming;
use hyper::{Request, Response};
use tokio::sync::watch;
use weather_api::{AdmittedMaintenanceArchiveOrigin, WeatherApiStartupPortError};

use crate::{
    NativeHttpBody, NativeHttpListenerConfiguration, NativeHttpListenerError,
    NativeHttpListenerReport, PreparedPrivateMaintenanceApi, PreparedPublicWeatherApi,
    WeatherApiMaintenancePool, WeatherApiPreparedListeners, WeatherApiSharedPool,
    serve_native_http1,
};

pub(crate) type NativeHttpServeFuture = Pin<
    Box<dyn Future<Output = Result<NativeHttpListenerReport, NativeHttpListenerError>> + 'static>,
>;

/// one primary and optional secondary listener outcome
pub(crate) struct CoordinatedNativeHttpListenerReports {
    pub primary: NativeHttpListenerReport,
    pub secondary: Option<NativeHttpListenerReport>,
}

/// one listener failure retained after its sibling has drained
pub(crate) enum CoordinatedNativeHttpListenerError {
    Primary(NativeHttpListenerError),
    Secondary(NativeHttpListenerError),
}

/// public API listener outcomes after every owned connection has drained
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeApiListenerReports {
    pub public: NativeHttpListenerReport,
    pub maintenance: Option<NativeHttpListenerReport>,
}

/// process-level listener ownership failures
#[derive(Debug)]
pub enum NativeApiListenerError {
    StartupFailed,
    MissingPublicListener,
    MissingMaintenanceListener,
    UnexpectedMaintenanceListener,
    PublicListenerInitialization(io::Error),
    MaintenanceListenerInitialization(io::Error),
    PublicListener(NativeHttpListenerError),
    MaintenanceListener(NativeHttpListenerError),
}

impl Display for NativeApiListenerError {
    /// describe the failed listener boundary
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartupFailed => formatter.write_str("native API listener startup failed"),
            Self::MissingPublicListener => {
                formatter.write_str("native API public listener was not started")
            }
            Self::MissingMaintenanceListener => {
                formatter.write_str("native API maintenance listener was not started")
            }
            Self::UnexpectedMaintenanceListener => formatter
                .write_str("native API maintenance listener requires dual-listener serving"),
            Self::PublicListenerInitialization(error) => {
                write!(
                    formatter,
                    "native API public listener initialization failed: {error}"
                )
            }
            Self::MaintenanceListenerInitialization(error) => write!(
                formatter,
                "native API maintenance listener initialization failed: {error}"
            ),
            Self::PublicListener(error) => {
                write!(formatter, "native API public listener failed: {error}")
            }
            Self::MaintenanceListener(error) => {
                write!(formatter, "native API maintenance listener failed: {error}")
            }
        }
    }
}

impl StdError for NativeApiListenerError {
    /// preserve the operating-system or transport failure
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::PublicListenerInitialization(error)
            | Self::MaintenanceListenerInitialization(error) => Some(error),
            Self::PublicListener(error) | Self::MaintenanceListener(error) => Some(error),
            Self::StartupFailed
            | Self::MissingPublicListener
            | Self::MissingMaintenanceListener
            | Self::UnexpectedMaintenanceListener => None,
        }
    }
}

struct BoundPublicApi<Pool, External> {
    listener: TcpListener,
    application: PreparedPublicWeatherApi<Pool, External>,
}

struct BoundMaintenanceApi<Pool> {
    listener: TcpListener,
    application: PreparedPrivateMaintenanceApi<Pool>,
}

/// sockets and prepared applications admitted for one native API process
pub struct NativeApiPreparedListeners<Pool, External> {
    bind_address: Ipv4Addr,
    public_configuration: NativeHttpListenerConfiguration,
    maintenance_configuration: NativeHttpListenerConfiguration,
    public: Option<BoundPublicApi<Pool, External>>,
    maintenance: Option<BoundMaintenanceApi<Pool>>,
    startup_failed: bool,
}

impl<Pool, External> NativeApiPreparedListeners<Pool, External> {
    /// construct production listeners on every IPv4 interface
    pub const fn new(
        public_configuration: NativeHttpListenerConfiguration,
        maintenance_configuration: NativeHttpListenerConfiguration,
    ) -> Self {
        Self::production(public_configuration, maintenance_configuration)
    }

    /// construct production listeners on every IPv4 interface
    pub const fn production(
        public_configuration: NativeHttpListenerConfiguration,
        maintenance_configuration: NativeHttpListenerConfiguration,
    ) -> Self {
        Self::on_address(
            Ipv4Addr::UNSPECIFIED,
            public_configuration,
            maintenance_configuration,
        )
    }

    /// construct isolated listeners on the IPv4 loopback interface
    pub const fn loopback(
        public_configuration: NativeHttpListenerConfiguration,
        maintenance_configuration: NativeHttpListenerConfiguration,
    ) -> Self {
        Self::on_address(
            Ipv4Addr::LOCALHOST,
            public_configuration,
            maintenance_configuration,
        )
    }

    /// retain one explicit bind address and independent listener limits
    const fn on_address(
        bind_address: Ipv4Addr,
        public_configuration: NativeHttpListenerConfiguration,
        maintenance_configuration: NativeHttpListenerConfiguration,
    ) -> Self {
        Self {
            bind_address,
            public_configuration,
            maintenance_configuration,
            public: None,
            maintenance: None,
            startup_failed: false,
        }
    }

    /// return the bound public address before serving begins
    pub fn public_local_addr(&self) -> Option<SocketAddr> {
        self.public
            .as_ref()
            .and_then(|bound| bound.listener.local_addr().ok())
    }

    /// return the bound maintenance address before serving begins
    pub fn maintenance_local_addr(&self) -> Option<SocketAddr> {
        self.maintenance
            .as_ref()
            .and_then(|bound| bound.listener.local_addr().ok())
    }

    /// serve one public listener until shutdown and drain every connection
    pub async fn serve_public<PublicHandler, PublicHandlerFuture, PublicHandlerError, Shutdown>(
        mut self,
        public_handler: PublicHandler,
        shutdown: Shutdown,
    ) -> Result<NativeApiListenerReports, NativeApiListenerError>
    where
        Pool: 'static,
        External: 'static,
        PublicHandler: Fn(
                Request<Incoming>,
                SocketAddr,
                Rc<RefCell<PreparedPublicWeatherApi<Pool, External>>>,
            ) -> PublicHandlerFuture
            + Clone
            + 'static,
        PublicHandlerFuture:
            Future<Output = Result<Response<NativeHttpBody>, PublicHandlerError>> + 'static,
        PublicHandlerError: Into<Box<dyn StdError + Send + Sync>> + 'static,
        Shutdown: Future<Output = ()>,
    {
        // reject every owner poisoned by an earlier bind or ordering failure
        if self.startup_failed {
            return Err(NativeApiListenerError::StartupFailed);
        }
        // require the public-first startup phase to have completed
        let public = self
            .public
            .take()
            .ok_or(NativeApiListenerError::MissingPublicListener)?;
        // require the caller to select dual-listener serving for retained maintenance state
        if self.maintenance.is_some() {
            return Err(NativeApiListenerError::UnexpectedMaintenanceListener);
        }
        let (stop_sender, stop_receiver) = native_http_listener_stop_fanout();
        let public = prepare_public_listener(
            public,
            self.public_configuration,
            public_handler,
            stop_receiver,
        )?;
        let reports = serve_coordinated_native_http_listeners(stop_sender, public, None, shutdown)
            .await
            .map_err(map_coordinated_api_error)?;
        Ok(NativeApiListenerReports {
            public: reports.primary,
            maintenance: None,
        })
    }

    /// serve public and maintenance listeners under one shutdown signal
    pub async fn serve_with_maintenance<
        PublicHandler,
        PublicHandlerFuture,
        PublicHandlerError,
        MaintenanceHandler,
        MaintenanceHandlerFuture,
        MaintenanceHandlerError,
        Shutdown,
    >(
        mut self,
        public_handler: PublicHandler,
        maintenance_handler: MaintenanceHandler,
        shutdown: Shutdown,
    ) -> Result<NativeApiListenerReports, NativeApiListenerError>
    where
        Pool: 'static,
        External: 'static,
        PublicHandler: Fn(
                Request<Incoming>,
                SocketAddr,
                Rc<RefCell<PreparedPublicWeatherApi<Pool, External>>>,
            ) -> PublicHandlerFuture
            + Clone
            + 'static,
        PublicHandlerFuture:
            Future<Output = Result<Response<NativeHttpBody>, PublicHandlerError>> + 'static,
        PublicHandlerError: Into<Box<dyn StdError + Send + Sync>> + 'static,
        MaintenanceHandler: Fn(
                Request<Incoming>,
                SocketAddr,
                Rc<PreparedPrivateMaintenanceApi<Pool>>,
            ) -> MaintenanceHandlerFuture
            + Clone
            + 'static,
        MaintenanceHandlerFuture:
            Future<Output = Result<Response<NativeHttpBody>, MaintenanceHandlerError>> + 'static,
        MaintenanceHandlerError: Into<Box<dyn StdError + Send + Sync>> + 'static,
        Shutdown: Future<Output = ()>,
    {
        // reject every owner poisoned by an earlier bind or ordering failure
        if self.startup_failed {
            return Err(NativeApiListenerError::StartupFailed);
        }
        // require both source-order startup phases before moving either application
        let public = self
            .public
            .take()
            .ok_or(NativeApiListenerError::MissingPublicListener)?;
        let maintenance = self
            .maintenance
            .take()
            .ok_or(NativeApiListenerError::MissingMaintenanceListener)?;
        let (stop_sender, stop_receiver) = native_http_listener_stop_fanout();
        let public = prepare_public_listener(
            public,
            self.public_configuration,
            public_handler,
            stop_receiver.clone(),
        )?;
        let maintenance = prepare_maintenance_listener(
            maintenance,
            self.maintenance_configuration,
            maintenance_handler,
            stop_receiver,
        )?;
        let reports = serve_coordinated_native_http_listeners(
            stop_sender,
            public,
            Some(maintenance),
            shutdown,
        )
        .await
        .map_err(map_coordinated_api_error)?;
        Ok(NativeApiListenerReports {
            public: reports.primary,
            maintenance: reports.secondary,
        })
    }

    /// bind one nonblocking standard socket before Tokio runtime ownership begins
    fn bind_listener(&self, port: NonZeroU16) -> io::Result<TcpListener> {
        let address = SocketAddr::new(IpAddr::V4(self.bind_address), port.get());
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    }

    /// drop every partially admitted endpoint and poison this startup owner
    fn fail_startup(&mut self) -> WeatherApiStartupPortError {
        self.public = None;
        self.maintenance = None;
        self.startup_failed = true;
        WeatherApiStartupPortError
    }
}

impl<Pool: WeatherApiSharedPool, External> NativeApiPreparedListeners<Pool, External> {
    /// Share the same retained public PostgreSQL pool for process shutdown.
    pub fn public_pool(&self) -> Option<Pool> {
        self.public
            .as_ref()
            .map(|bound| bound.application.shared_pool())
    }
}

impl<Pool: WeatherApiMaintenancePool + WeatherApiSharedPool, External>
    NativeApiPreparedListeners<Pool, External>
{
    /// Borrow the already admitted private pool and archive origin without rereading configuration.
    pub fn maintenance_runtime(&self) -> Option<(Pool, AdmittedMaintenanceArchiveOrigin)> {
        self.maintenance.as_ref().map(|bound| {
            (
                bound.application.shared_pool(),
                bound
                    .application
                    .capture_material()
                    .0
                    .configuration
                    .archive_origin
                    .clone(),
            )
        })
    }
}

impl<Pool, External> WeatherApiPreparedListeners<Pool, External>
    for NativeApiPreparedListeners<Pool, External>
{
    /// bind and retain the public application exactly once
    fn listen_public(
        &mut self,
        port: NonZeroU16,
        application: PreparedPublicWeatherApi<Pool, External>,
    ) -> Result<(), WeatherApiStartupPortError> {
        // reject duplicate, reversed, or previously failed startup attempts
        if self.startup_failed || self.public.is_some() || self.maintenance.is_some() {
            return Err(self.fail_startup());
        }
        let listener = match self.bind_listener(port) {
            Ok(listener) => listener,
            Err(_) => return Err(self.fail_startup()),
        };
        self.public = Some(BoundPublicApi {
            listener,
            application,
        });
        Ok(())
    }

    /// bind maintenance only after public startup and retain its application exactly once
    fn listen_maintenance(
        &mut self,
        port: NonZeroU16,
        application: PreparedPrivateMaintenanceApi<Pool>,
    ) -> Result<(), WeatherApiStartupPortError> {
        // reject missing public, duplicate, or previously failed startup attempts
        if self.startup_failed || self.public.is_none() || self.maintenance.is_some() {
            return Err(self.fail_startup());
        }
        let listener = match self.bind_listener(port) {
            Ok(listener) => listener,
            // release the partial public bind before returning the maintenance failure
            Err(_) => return Err(self.fail_startup()),
        };
        self.maintenance = Some(BoundMaintenanceApi {
            listener,
            application,
        });
        Ok(())
    }
}

/// convert the retained public socket and app into one local listener future
fn prepare_public_listener<Pool, External, Handler, HandlerFuture, HandlerError>(
    bound: BoundPublicApi<Pool, External>,
    configuration: NativeHttpListenerConfiguration,
    handler: Handler,
    stop: watch::Receiver<bool>,
) -> Result<NativeHttpServeFuture, NativeApiListenerError>
where
    Pool: 'static,
    External: 'static,
    Handler: Fn(
            Request<Incoming>,
            SocketAddr,
            Rc<RefCell<PreparedPublicWeatherApi<Pool, External>>>,
        ) -> HandlerFuture
        + Clone
        + 'static,
    HandlerFuture: Future<Output = Result<Response<NativeHttpBody>, HandlerError>> + 'static,
    HandlerError: Into<Box<dyn StdError + Send + Sync>> + 'static,
{
    let listener = tokio::net::TcpListener::from_std(bound.listener)
        .map_err(NativeApiListenerError::PublicListenerInitialization)?;
    let application = Rc::new(RefCell::new(bound.application));
    let listener_handler = move |request, peer| handler(request, peer, Rc::clone(&application));
    Ok(Box::pin(serve_native_http1(
        listener,
        configuration,
        listener_handler,
        wait_for_native_http_stop(stop),
    )))
}

/// convert the retained maintenance socket and app into one local listener future
fn prepare_maintenance_listener<Pool, Handler, HandlerFuture, HandlerError>(
    bound: BoundMaintenanceApi<Pool>,
    configuration: NativeHttpListenerConfiguration,
    handler: Handler,
    stop: watch::Receiver<bool>,
) -> Result<NativeHttpServeFuture, NativeApiListenerError>
where
    Pool: 'static,
    Handler: Fn(Request<Incoming>, SocketAddr, Rc<PreparedPrivateMaintenanceApi<Pool>>) -> HandlerFuture
        + Clone
        + 'static,
    HandlerFuture: Future<Output = Result<Response<NativeHttpBody>, HandlerError>> + 'static,
    HandlerError: Into<Box<dyn StdError + Send + Sync>> + 'static,
{
    let listener = tokio::net::TcpListener::from_std(bound.listener)
        .map_err(NativeApiListenerError::MaintenanceListenerInitialization)?;
    let application = Rc::new(bound.application);
    let listener_handler = move |request, peer| handler(request, peer, Rc::clone(&application));
    Ok(Box::pin(serve_native_http1(
        listener,
        configuration,
        listener_handler,
        wait_for_native_http_stop(stop),
    )))
}

/// map shared listener positions onto API endpoint names
fn map_coordinated_api_error(error: CoordinatedNativeHttpListenerError) -> NativeApiListenerError {
    match error {
        CoordinatedNativeHttpListenerError::Primary(error) => {
            NativeApiListenerError::PublicListener(error)
        }
        CoordinatedNativeHttpListenerError::Secondary(error) => {
            NativeApiListenerError::MaintenanceListener(error)
        }
    }
}

/// wait for the coordinator's single fanout signal
pub(crate) async fn wait_for_native_http_stop(mut stop: watch::Receiver<bool>) {
    // avoid parking after a sibling completed before this listener began polling
    if *stop.borrow() {
        return;
    }
    let _sender_closed_after_completion = stop.changed().await;
}

/// signal all retained listeners without allocating per-listener shutdown work
fn signal_native_http_stop(stop: &watch::Sender<bool>) {
    // publish only the single false-to-true transition
    if !*stop.borrow() {
        let _receivers_already_closed = stop.send(true);
    }
}

/// create the single watch fanout shared by every process listener
pub(crate) fn native_http_listener_stop_fanout() -> (watch::Sender<bool>, watch::Receiver<bool>) {
    watch::channel(false)
}

/// drive listener futures together and reap both after shutdown or either failure
pub(crate) async fn serve_coordinated_native_http_listeners<Shutdown>(
    stop_sender: watch::Sender<bool>,
    primary: NativeHttpServeFuture,
    secondary: Option<NativeHttpServeFuture>,
    shutdown: Shutdown,
) -> Result<CoordinatedNativeHttpListenerReports, CoordinatedNativeHttpListenerError>
where
    Shutdown: Future<Output = ()>,
{
    let mut primary = Some(primary);
    let mut secondary = secondary;
    let mut shutdown = Some(Box::pin(shutdown));
    let mut primary_report = None;
    let mut secondary_report = None;
    let mut first_error = None;

    poll_fn(|context| {
        // fan out the process signal before admitting more listener progress
        if let Some(future) = shutdown.as_mut()
            && future.as_mut().poll(context).is_ready()
        {
            shutdown = None;
            signal_native_http_stop(&stop_sender);
        }
        // retain the primary result and stop the sibling on every terminal failure
        if let Some(future) = primary.as_mut()
            && let Poll::Ready(result) = future.as_mut().poll(context)
        {
            primary = None;
            shutdown = None;
            match result {
                Ok(report) => primary_report = Some(report),
                Err(error) => {
                    // preserve the first listener failure while still reaping its sibling
                    if first_error.is_none() {
                        first_error = Some(CoordinatedNativeHttpListenerError::Primary(error));
                    }
                }
            }
            signal_native_http_stop(&stop_sender);
        }
        // retain the secondary result and stop the sibling on every terminal failure
        if let Some(future) = secondary.as_mut()
            && let Poll::Ready(result) = future.as_mut().poll(context)
        {
            secondary = None;
            shutdown = None;
            match result {
                Ok(report) => secondary_report = Some(report),
                Err(error) => {
                    // preserve the first listener failure while still reaping its sibling
                    if first_error.is_none() {
                        first_error = Some(CoordinatedNativeHttpListenerError::Secondary(error));
                    }
                }
            }
            signal_native_http_stop(&stop_sender);
        }
        // return only after every present listener future reached a terminal result
        if primary.is_none() && secondary.is_none() {
            // surface listener failure only after its sibling has drained
            if let Some(error) = first_error.take() {
                return Poll::Ready(Err(error));
            }
            let primary = primary_report
                .take()
                .expect("completed primary listener must retain its report");
            return Poll::Ready(Ok(CoordinatedNativeHttpListenerReports {
                primary,
                secondary: secondary_report.take(),
            }));
        }
        Poll::Pending
    })
    .await
}
