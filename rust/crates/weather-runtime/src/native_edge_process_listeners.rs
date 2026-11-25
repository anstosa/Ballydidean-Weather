//! Concrete socket and application ownership for the native edge process.

use std::cell::RefCell;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::future::{Future, poll_fn};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::num::NonZeroU16;
use std::rc::Rc;
use std::task::Poll;

use hyper::body::Incoming;
use hyper::{Request, Response};
use tokio::sync::watch;
use weather_edge::{
    EdgeStartupArchive, EdgeStartupBootError, EdgeStartupLifecycle,
    EdgeStartupLifecycleConfiguration, EdgeStartupLifecycleParts, EdgeStartupLifecyclePorts,
    EdgeStartupPublicOwnerView, ForecastSchedulerConfiguration,
};

use crate::native_api_process_listeners::{
    CoordinatedNativeHttpListenerError, NativeHttpServeFuture, native_http_listener_stop_fanout,
    serve_coordinated_native_http_listeners, wait_for_native_http_stop,
};
use crate::{
    NativeForecastSchedulerLane, NativeHttpBody, NativeHttpConnectionScope,
    NativeHttpListenerConfiguration, NativeHttpListenerError, NativeHttpListenerReport,
    NativeTrackedHttpBody, serve_native_edge_http1, serve_native_http1,
};

/// resource initialization retained behind the native edge socket adapter.
pub trait NativeEdgeStartupResources {
    type Site: 'static;
    type Admin: 'static;
    type Evidence: 'static;
    type RevisionArchive: 'static;
    type Archive: 'static;
    type Scheduler: NativeForecastSchedulerLane + 'static;
    type Assets: 'static;
    type Error;

    /// load optional site metadata before constructing the admin owner.
    fn load_site(&mut self) -> Option<Self::Site>;

    /// construct the admin owner with the optional site result.
    fn initialize_admin(&mut self, site: Option<&Self::Site>) -> Result<Self::Admin, Self::Error>;

    /// initialize evidence capture without failing ordinary serving.
    fn initialize_evidence(&mut self) -> Self::Evidence;

    /// construct the private revision archive owner.
    fn initialize_revision_archive(&mut self) -> Result<Self::RevisionArchive, Self::Error>;

    /// initialize the optional online archive owner.
    fn initialize_archive(&mut self) -> Result<Self::Archive, Self::Error>;

    /// initialize scheduler state before either listener request.
    fn initialize_scheduler(
        &mut self,
        configuration: ForecastSchedulerConfiguration,
    ) -> Self::Scheduler;

    /// load the fixed public asset inventory.
    fn load_assets(&mut self) -> Result<Self::Assets, Self::Error>;

    /// start the real scheduler from the one public-listener readiness event.
    fn start_scheduler(&mut self, scheduler: &mut Self::Scheduler) -> Result<bool, Self::Error>;
}

/// startup errors introduced by concrete socket ownership.
#[derive(Debug)]
pub enum NativeEdgeStartupError<Error> {
    Resource(Error),
    MissingPrivatePort,
    Bind(io::Error),
}

impl<Error: Display> Display for NativeEdgeStartupError<Error> {
    /// describe the refused resource or socket operation.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resource(error) => write!(formatter, "native edge resource: {error}"),
            Self::MissingPrivatePort => {
                formatter.write_str("native edge private listener has no configured port")
            }
            Self::Bind(error) => write!(formatter, "native edge listener bind: {error}"),
        }
    }
}

impl<Error: StdError + 'static> StdError for NativeEdgeStartupError<Error> {
    /// preserve the exact resource or operating-system refusal.
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Resource(error) => Some(error),
            Self::Bind(error) => Some(error),
            Self::MissingPrivatePort => None,
        }
    }
}

/// one standard listener retained until readiness transfers it to Tokio.
pub struct NativeEdgeBoundListener {
    listener: Option<TcpListener>,
}

impl NativeEdgeBoundListener {
    /// return the bound address before serving begins.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener
            .as_ref()
            .ok_or_else(listener_closed)
            .and_then(TcpListener::local_addr)
    }

    /// move the admitted standard socket into its runtime listener.
    fn into_listener(mut self) -> Option<TcpListener> {
        self.listener.take()
    }
}

/// concrete public-first edge listener ports over one real resource owner.
pub struct NativeEdgeStartupPorts<Resources> {
    resources: Resources,
    bind_address: Ipv4Addr,
    public_port: NonZeroU16,
    private_port: Option<NonZeroU16>,
}

impl<Resources> NativeEdgeStartupPorts<Resources> {
    /// construct production listeners on every IPv4 interface.
    pub const fn production(
        resources: Resources,
        public_port: NonZeroU16,
        private_port: Option<NonZeroU16>,
    ) -> Self {
        Self::on_address(resources, Ipv4Addr::UNSPECIFIED, public_port, private_port)
    }

    /// construct isolated listeners on the IPv4 loopback interface.
    pub const fn loopback(
        resources: Resources,
        public_port: NonZeroU16,
        private_port: Option<NonZeroU16>,
    ) -> Self {
        Self::on_address(resources, Ipv4Addr::LOCALHOST, public_port, private_port)
    }

    /// retain an explicit bind address and both source listener ports.
    const fn on_address(
        resources: Resources,
        bind_address: Ipv4Addr,
        public_port: NonZeroU16,
        private_port: Option<NonZeroU16>,
    ) -> Self {
        Self {
            resources,
            bind_address,
            public_port,
            private_port,
        }
    }

    /// bind one nonblocking standard socket before Tokio owns admission.
    fn bind_listener(&self, port: NonZeroU16) -> io::Result<NativeEdgeBoundListener> {
        let address = SocketAddr::new(IpAddr::V4(self.bind_address), port.get());
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(NativeEdgeBoundListener {
            listener: Some(listener),
        })
    }
}

impl<Resources: NativeEdgeStartupResources> EdgeStartupLifecyclePorts
    for NativeEdgeStartupPorts<Resources>
{
    type Site = Resources::Site;
    type Admin = Resources::Admin;
    type Evidence = Resources::Evidence;
    type RevisionArchive = Resources::RevisionArchive;
    type Archive = Resources::Archive;
    type Scheduler = Resources::Scheduler;
    type Assets = Resources::Assets;
    type PublicListener = NativeEdgeBoundListener;
    type PrivateListener = NativeEdgeBoundListener;
    type Error = NativeEdgeStartupError<Resources::Error>;

    /// delegate optional site loading to the real resource owner.
    fn load_site(&mut self) -> Option<Self::Site> {
        self.resources.load_site()
    }

    /// delegate admin initialization after site loading.
    fn initialize_admin(&mut self, site: Option<&Self::Site>) -> Result<Self::Admin, Self::Error> {
        self.resources
            .initialize_admin(site)
            .map_err(NativeEdgeStartupError::Resource)
    }

    /// delegate contained evidence initialization.
    fn initialize_evidence(&mut self) -> Self::Evidence {
        self.resources.initialize_evidence()
    }

    /// delegate revision archive initialization.
    fn initialize_revision_archive(&mut self) -> Result<Self::RevisionArchive, Self::Error> {
        self.resources
            .initialize_revision_archive()
            .map_err(NativeEdgeStartupError::Resource)
    }

    /// delegate optional online archive initialization.
    fn initialize_archive(&mut self) -> Result<Self::Archive, Self::Error> {
        self.resources
            .initialize_archive()
            .map_err(NativeEdgeStartupError::Resource)
    }

    /// delegate scheduler initialization without starting its timer.
    fn initialize_scheduler(
        &mut self,
        configuration: ForecastSchedulerConfiguration,
    ) -> Self::Scheduler {
        self.resources.initialize_scheduler(configuration)
    }

    /// delegate fixed public asset loading.
    fn load_assets(&mut self) -> Result<Self::Assets, Self::Error> {
        self.resources
            .load_assets()
            .map_err(NativeEdgeStartupError::Resource)
    }

    /// bind public before the lifecycle can request a private socket.
    fn request_public_listener(
        &mut self,
        _owners: EdgeStartupPublicOwnerView<'_, Self>,
    ) -> Result<Self::PublicListener, Self::Error> {
        self.bind_listener(self.public_port)
            .map_err(NativeEdgeStartupError::Bind)
    }

    /// bind the optional private socket after public admission.
    fn request_private_listener(
        &mut self,
        _revision: &Self::RevisionArchive,
    ) -> Result<Self::PrivateListener, Self::Error> {
        let port = self
            .private_port
            .ok_or(NativeEdgeStartupError::MissingPrivatePort)?;
        self.bind_listener(port)
            .map_err(NativeEdgeStartupError::Bind)
    }

    /// delegate the one real scheduler start after both requests returned.
    fn start_scheduler(&mut self, scheduler: &mut Self::Scheduler) -> Result<bool, Self::Error> {
        self.resources
            .start_scheduler(scheduler)
            .map_err(NativeEdgeStartupError::Resource)
    }

    /// close the retained public socket without manufacturing background work.
    fn close_public_listener(
        &mut self,
        listener: &mut Self::PublicListener,
    ) -> Result<(), Self::Error> {
        listener.listener = None;
        Ok(())
    }

    /// close the retained private socket without manufacturing background work.
    fn close_private_listener(
        &mut self,
        listener: &mut Self::PrivateListener,
    ) -> Result<(), Self::Error> {
        listener.listener = None;
        Ok(())
    }
}

/// public edge owners shared only between requests admitted on that socket.
pub struct NativeEdgePublicApplication<Resources: NativeEdgeStartupResources> {
    pub site: Option<Resources::Site>,
    pub admin: Resources::Admin,
    pub evidence: Resources::Evidence,
    pub archive: EdgeStartupArchive<Resources::Archive>,
    scheduler: Rc<RefCell<Resources::Scheduler>>,
    pub assets: Resources::Assets,
}

impl<Resources: NativeEdgeStartupResources> NativeEdgePublicApplication<Resources> {
    /// Inspect scheduler status through a short synchronous borrow; no reference can escape.
    pub fn inspect_scheduler<Output>(
        &self,
        inspect: impl FnOnce(&Resources::Scheduler) -> Output,
    ) -> Output {
        inspect(&self.scheduler.borrow())
    }
}

/// private archive owner shared only between requests admitted on that socket.
pub struct NativeEdgePrivateApplication<Resources: NativeEdgeStartupResources> {
    pub revision_archive: Resources::RevisionArchive,
}

/// edge listener outcomes after every owned connection has drained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeEdgeProcessReports {
    pub public: NativeHttpListenerReport,
    pub private: Option<NativeHttpListenerReport>,
}

/// process-level startup, conversion, and listener ownership failures.
#[derive(Debug)]
pub enum NativeEdgeProcessError<Error> {
    Boot(EdgeStartupBootError<NativeEdgeStartupError<Error>>),
    Readiness(NativeEdgeStartupError<Error>),
    MissingPublicListener,
    MissingPrivateListener,
    UnexpectedPrivateListener,
    PublicListenerInitialization(io::Error),
    PrivateListenerInitialization(io::Error),
    PublicListener(NativeHttpListenerError),
    PrivateListener(NativeHttpListenerError),
}

impl<Error: Display> Display for NativeEdgeProcessError<Error> {
    /// describe the exact process ownership boundary that failed.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boot(error) => write!(
                formatter,
                "native edge boot {:?}: {}",
                error.phase, error.error
            ),
            Self::Readiness(error) => write!(formatter, "native edge readiness: {error}"),
            Self::MissingPublicListener => {
                formatter.write_str("native edge public listener was not started")
            }
            Self::MissingPrivateListener => {
                formatter.write_str("native edge private listener was not started")
            }
            Self::UnexpectedPrivateListener => {
                formatter.write_str("native edge private listener requires dual-listener serving")
            }
            Self::PublicListenerInitialization(error) => write!(
                formatter,
                "native edge public listener initialization failed: {error}"
            ),
            Self::PrivateListenerInitialization(error) => write!(
                formatter,
                "native edge private listener initialization failed: {error}"
            ),
            Self::PublicListener(error) => {
                write!(formatter, "native edge public listener failed: {error}")
            }
            Self::PrivateListener(error) => {
                write!(formatter, "native edge private listener failed: {error}")
            }
        }
    }
}

impl<Error: StdError + 'static> StdError for NativeEdgeProcessError<Error> {
    /// preserve the resource, operating-system, or transport failure.
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Boot(error) => Some(&error.error),
            Self::Readiness(error) => Some(error),
            Self::PublicListenerInitialization(error)
            | Self::PrivateListenerInitialization(error) => Some(error),
            Self::PublicListener(error) | Self::PrivateListener(error) => Some(error),
            Self::MissingPublicListener
            | Self::MissingPrivateListener
            | Self::UnexpectedPrivateListener => None,
        }
    }
}

/// bound native edge process awaiting runtime readiness and request handlers.
pub struct NativeEdgePreparedProcess<Resources: NativeEdgeStartupResources> {
    lifecycle: EdgeStartupLifecycle<NativeEdgeStartupPorts<Resources>>,
    public_configuration: NativeHttpListenerConfiguration,
    private_configuration: NativeHttpListenerConfiguration,
}

impl<Resources: NativeEdgeStartupResources> NativeEdgePreparedProcess<Resources> {
    /// initialize real resources and bind public then optional private sockets.
    pub fn initialize(
        ports: NativeEdgeStartupPorts<Resources>,
        lifecycle_configuration: EdgeStartupLifecycleConfiguration,
        public_configuration: NativeHttpListenerConfiguration,
        private_configuration: NativeHttpListenerConfiguration,
    ) -> Result<Self, NativeEdgeProcessError<Resources::Error>> {
        let lifecycle = EdgeStartupLifecycle::initialize(ports, lifecycle_configuration)
            .map_err(NativeEdgeProcessError::Boot)?;
        Ok(Self {
            lifecycle,
            public_configuration,
            private_configuration,
        })
    }

    /// return the bound public address before serving begins.
    pub fn public_local_addr(&self) -> Option<SocketAddr> {
        self.lifecycle.public_listener().local_addr().ok()
    }

    /// return the optional bound private address before serving begins.
    pub fn private_local_addr(&self) -> Option<SocketAddr> {
        self.lifecycle
            .private_listener()
            .and_then(|listener| listener.local_addr().ok())
    }

    /// serve only the public edge until shutdown and drain every connection.
    /// the caller drives this future with the time-enabled NativeBackendRuntime.
    pub async fn serve_public<PublicHandler, PublicFuture, PublicError, Shutdown>(
        self,
        public_handler: PublicHandler,
        shutdown: Shutdown,
    ) -> Result<NativeEdgeProcessReports, NativeEdgeProcessError<Resources::Error>>
    where
        Resources: 'static,
        PublicHandler: Fn(
                Request<Incoming>,
                SocketAddr,
                NativeHttpConnectionScope,
                Rc<RefCell<NativeEdgePublicApplication<Resources>>>,
            ) -> PublicFuture
            + Clone
            + 'static,
        PublicFuture:
            Future<Output = Result<Response<NativeTrackedHttpBody>, PublicError>> + 'static,
        PublicError: Into<Box<dyn StdError + Send + Sync>> + 'static,
        Shutdown: Future<Output = ()>,
    {
        let serving = self.into_serving(false)?;
        let (stop_sender, stop_receiver) = native_http_listener_stop_fanout();
        let public = prepare_public_listener(
            serving.public_listener,
            serving.public_application,
            serving.public_configuration,
            public_handler,
            stop_receiver,
        )?;
        let reports = serve_coordinated_native_http_listeners(stop_sender, public, None, shutdown)
            .await
            .map_err(map_coordinated_edge_error)?;
        Ok(NativeEdgeProcessReports {
            public: reports.primary,
            private: None,
        })
    }

    /// serve public and private edge listeners under one shutdown signal.
    /// the caller drives this future with the time-enabled NativeBackendRuntime.
    pub async fn serve_with_private<
        PublicHandler,
        PublicFuture,
        PublicError,
        PrivateHandler,
        PrivateFuture,
        PrivateError,
        Shutdown,
    >(
        self,
        public_handler: PublicHandler,
        private_handler: PrivateHandler,
        shutdown: Shutdown,
    ) -> Result<NativeEdgeProcessReports, NativeEdgeProcessError<Resources::Error>>
    where
        Resources: 'static,
        PublicHandler: Fn(
                Request<Incoming>,
                SocketAddr,
                NativeHttpConnectionScope,
                Rc<RefCell<NativeEdgePublicApplication<Resources>>>,
            ) -> PublicFuture
            + Clone
            + 'static,
        PublicFuture:
            Future<Output = Result<Response<NativeTrackedHttpBody>, PublicError>> + 'static,
        PublicError: Into<Box<dyn StdError + Send + Sync>> + 'static,
        PrivateHandler: Fn(
                Request<Incoming>,
                SocketAddr,
                Rc<RefCell<NativeEdgePrivateApplication<Resources>>>,
            ) -> PrivateFuture
            + Clone
            + 'static,
        PrivateFuture: Future<Output = Result<Response<NativeHttpBody>, PrivateError>> + 'static,
        PrivateError: Into<Box<dyn StdError + Send + Sync>> + 'static,
        Shutdown: Future<Output = ()>,
    {
        let serving = self.into_serving(true)?;
        let private_listener = serving
            .private_listener
            .ok_or(NativeEdgeProcessError::MissingPrivateListener)?;
        let (stop_sender, stop_receiver) = native_http_listener_stop_fanout();
        let public = prepare_public_listener(
            serving.public_listener,
            serving.public_application,
            serving.public_configuration,
            public_handler,
            stop_receiver.clone(),
        )?;
        let private = prepare_private_listener(
            private_listener,
            serving.private_application,
            serving.private_configuration,
            private_handler,
            stop_receiver,
        )?;
        let reports =
            serve_coordinated_native_http_listeners(stop_sender, public, Some(private), shutdown)
                .await
                .map_err(map_coordinated_edge_error)?;
        Ok(NativeEdgeProcessReports {
            public: reports.primary,
            private: reports.secondary,
        })
    }

    /// fire readiness once and split the lifecycle into socket-specific owners.
    fn into_serving(
        self,
        private_required: bool,
    ) -> Result<NativeEdgeServing<Resources>, NativeEdgeProcessError<Resources::Error>> {
        let parts = self.into_composition_parts(private_required)?;
        Ok(NativeEdgeServing {
            public_listener: parts.public_listener,
            private_listener: parts.private_listener,
            public_application: NativeEdgePublicApplication {
                site: parts.site,
                admin: parts.admin,
                evidence: parts.evidence,
                archive: parts.archive,
                scheduler: Rc::new(RefCell::new(parts.scheduler)),
                assets: parts.assets,
            },
            private_application: NativeEdgePrivateApplication {
                revision_archive: parts.revision_archive,
            },
            public_configuration: parts.public_configuration,
            private_configuration: parts.private_configuration,
        })
    }

    /// retain the initialized resources and transfer the complete lifecycle to composition.
    pub(crate) fn into_composition_parts(
        mut self,
        private_required: bool,
    ) -> Result<NativeEdgeCompositionParts<Resources>, NativeEdgeProcessError<Resources::Error>>
    {
        // reject the wrong serving shape before starting the independent scheduler
        if private_required && self.lifecycle.private_listener().is_none() {
            return Err(NativeEdgeProcessError::MissingPrivateListener);
        }
        // require the public-only entry point when no private app was requested
        if !private_required && self.lifecycle.private_listener().is_some() {
            return Err(NativeEdgeProcessError::UnexpectedPrivateListener);
        }
        self.lifecycle
            .public_listener_ready()
            .map_err(NativeEdgeProcessError::Readiness)?;
        let EdgeStartupLifecycleParts {
            ports,
            site,
            admin,
            evidence,
            revision_archive,
            archive,
            scheduler,
            assets,
            public_listener,
            private_listener,
            scheduler_readiness: _,
        } = self.lifecycle.into_parts();
        let public_listener = public_listener
            .into_listener()
            .ok_or(NativeEdgeProcessError::MissingPublicListener)?;
        // preserve the disabled listener as absent while rejecting a closed requested socket
        let private_listener = match private_listener {
            Some(listener) => Some(
                listener
                    .into_listener()
                    .ok_or(NativeEdgeProcessError::MissingPrivateListener)?,
            ),
            None => None,
        };
        Ok(NativeEdgeCompositionParts {
            resources: ports.resources,
            site,
            admin,
            evidence,
            revision_archive,
            archive,
            scheduler,
            assets,
            public_listener,
            private_listener,
            public_configuration: self.public_configuration,
            private_configuration: self.private_configuration,
        })
    }
}

/// complete initialized owners transferred without reopening resources or listeners.
pub(crate) struct NativeEdgeCompositionParts<Resources: NativeEdgeStartupResources> {
    pub(crate) resources: Resources,
    pub(crate) site: Option<Resources::Site>,
    pub(crate) admin: Resources::Admin,
    pub(crate) evidence: Resources::Evidence,
    pub(crate) revision_archive: Resources::RevisionArchive,
    pub(crate) archive: EdgeStartupArchive<Resources::Archive>,
    pub(crate) scheduler: Resources::Scheduler,
    pub(crate) assets: Resources::Assets,
    pub(crate) public_listener: TcpListener,
    pub(crate) private_listener: Option<TcpListener>,
    pub(crate) public_configuration: NativeHttpListenerConfiguration,
    pub(crate) private_configuration: NativeHttpListenerConfiguration,
}

struct NativeEdgeServing<Resources: NativeEdgeStartupResources> {
    public_listener: TcpListener,
    private_listener: Option<TcpListener>,
    public_application: NativeEdgePublicApplication<Resources>,
    private_application: NativeEdgePrivateApplication<Resources>,
    public_configuration: NativeHttpListenerConfiguration,
    private_configuration: NativeHttpListenerConfiguration,
}

/// convert the public socket and app into one coordinated listener future.
fn prepare_public_listener<Resources, Handler, HandlerFuture, HandlerError>(
    listener: TcpListener,
    application: NativeEdgePublicApplication<Resources>,
    configuration: NativeHttpListenerConfiguration,
    handler: Handler,
    stop: watch::Receiver<bool>,
) -> Result<NativeHttpServeFuture, NativeEdgeProcessError<Resources::Error>>
where
    Resources: NativeEdgeStartupResources + 'static,
    Handler: Fn(
            Request<Incoming>,
            SocketAddr,
            NativeHttpConnectionScope,
            Rc<RefCell<NativeEdgePublicApplication<Resources>>>,
        ) -> HandlerFuture
        + Clone
        + 'static,
    HandlerFuture: Future<Output = Result<Response<NativeTrackedHttpBody>, HandlerError>> + 'static,
    HandlerError: Into<Box<dyn StdError + Send + Sync>> + 'static,
{
    let listener = tokio::net::TcpListener::from_std(listener)
        .map_err(NativeEdgeProcessError::PublicListenerInitialization)?;
    // the timer lane owns its own borrow cell and never borrows the aggregate request application
    let scheduler = Rc::clone(&application.scheduler);
    let application = Rc::new(RefCell::new(application));
    let scheduler_stop = stop.clone();
    let listener_handler =
        move |request, peer, scope| handler(request, peer, scope, Rc::clone(&application));
    let mut listener = Box::pin(serve_native_edge_http1(
        listener,
        configuration,
        listener_handler,
        wait_for_native_http_stop(stop),
    ));
    Ok(Box::pin(async move {
        let mut scheduler_active = true;
        let result = poll_fn(|context| {
            // prioritize socket progress and make the timer unreferenced when the public listener ends
            if let Poll::Ready(result) = listener.as_mut().poll(context) {
                return Poll::Ready(result);
            }
            // do not initiate new due work after the shared process stop has fired
            if scheduler_active && !*scheduler_stop.borrow() {
                // scheduler ownership is independent of every request's application borrow
                scheduler_active = scheduler.borrow_mut().poll_scheduler(context).is_pending();
            }
            Poll::Pending
        })
        .await;
        // release the scheduler lane along with its public process owner
        scheduler.borrow_mut().stop_scheduler();
        // retain the runtime until any already admitted background due observation exits
        poll_fn(|context| scheduler.borrow_mut().poll_drained(context)).await;
        result
    }))
}

/// convert the private socket and app into one coordinated listener future.
fn prepare_private_listener<Resources, Handler, HandlerFuture, HandlerError>(
    listener: TcpListener,
    application: NativeEdgePrivateApplication<Resources>,
    configuration: NativeHttpListenerConfiguration,
    handler: Handler,
    stop: watch::Receiver<bool>,
) -> Result<NativeHttpServeFuture, NativeEdgeProcessError<Resources::Error>>
where
    Resources: NativeEdgeStartupResources + 'static,
    Handler: Fn(
            Request<Incoming>,
            SocketAddr,
            Rc<RefCell<NativeEdgePrivateApplication<Resources>>>,
        ) -> HandlerFuture
        + Clone
        + 'static,
    HandlerFuture: Future<Output = Result<Response<NativeHttpBody>, HandlerError>> + 'static,
    HandlerError: Into<Box<dyn StdError + Send + Sync>> + 'static,
{
    let listener = tokio::net::TcpListener::from_std(listener)
        .map_err(NativeEdgeProcessError::PrivateListenerInitialization)?;
    let application = Rc::new(RefCell::new(application));
    let listener_handler = move |request, peer| handler(request, peer, Rc::clone(&application));
    Ok(Box::pin(serve_native_http1(
        listener,
        configuration,
        listener_handler,
        wait_for_native_http_stop(stop),
    )))
}

/// map shared listener positions onto public and private edge names.
fn map_coordinated_edge_error<Error>(
    error: CoordinatedNativeHttpListenerError,
) -> NativeEdgeProcessError<Error> {
    match error {
        CoordinatedNativeHttpListenerError::Primary(error) => {
            NativeEdgeProcessError::PublicListener(error)
        }
        CoordinatedNativeHttpListenerError::Secondary(error) => {
            NativeEdgeProcessError::PrivateListener(error)
        }
    }
}

/// retain a closed standard listener as an ordinary I/O error.
fn listener_closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "native edge listener is closed",
    )
}
