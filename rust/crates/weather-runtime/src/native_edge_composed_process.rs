//! Pre-poll composition and coordinated serving for the complete native edge process.

use std::{
    cell::RefCell,
    fmt,
    future::{Future, poll_fn},
    net::TcpListener,
    num::NonZeroUsize,
    rc::Rc,
    sync::Arc,
    task::Poll,
    time::Duration,
};

use hyper::{Request, body::Incoming};
use weather_edge::{EdgeStartupArchive, ForecastCycleHashPair, ForecastEvidenceReceiptCodec};

use crate::native_api_process_listeners::{
    CoordinatedNativeHttpListenerError, NativeHttpServeFuture, native_http_listener_stop_fanout,
    serve_coordinated_native_http_listeners, wait_for_native_http_stop,
};
use crate::native_edge_process_listeners::NativeEdgeCompositionParts;
use crate::{
    NativeEdgeAdminAdmissionOwner, NativeEdgePostResponseArchive, NativeEdgePostResponseLane,
    NativeEdgePreparedProcess, NativeEdgePrivateHandler, NativeEdgeProcessError,
    NativeEdgeProcessReports, NativeEdgePublicHandler, NativeEdgePublicRouteGraph,
    NativeEdgePublicRouteGraphError, NativeEdgeRevisionArchive, NativeEdgeRouteExecution,
    NativeEdgeRuntimeEvidence, NativeEdgeRuntimeOnlineArchive, NativeEdgeRuntimeResources,
    NativeEdgeRuntimeResourcesError, NativeEdgeRuntimeScheduler, NativeForecastSchedulerLane,
    NativeHttpConnectionScope, NativeHttpListenerConfiguration, NativeHttpListenerError,
    serve_native_edge_http1, serve_native_http1,
};

const PUBLIC_ROUTE_WORKERS: usize = 8;

/// Public request owner specialized to the admitted evidence codec and filesystem accounting port.
pub type NativeEdgeComposedPublicHandler<Codec, Statfs> = NativeEdgePublicHandler<
    NativeEdgeRuntimeEvidence<Codec, Statfs>,
    NativeEdgeRuntimeOnlineArchive<Codec, Statfs>,
>;

/// Fully composed native edge owners awaiting the first listener poll.
pub struct NativeEdgeComposedProcess<Codec, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec,
    Statfs: weather_edge::ForecastAllocationStatfsPort,
{
    public_listener: TcpListener,
    private_listener: Option<TcpListener>,
    public_configuration: NativeHttpListenerConfiguration,
    private_configuration: NativeHttpListenerConfiguration,
    public_handler: NativeEdgeComposedPublicHandler<Codec, Statfs>,
    private_handler: Option<NativeEdgePrivateHandler<NativeEdgeRevisionArchive>>,
    scheduler: NativeEdgeRuntimeScheduler<Codec, Statfs>,
    channel_bound_capture_active: bool,
}

/// Composition, listener, or post-response drain failure for the real edge process.
#[derive(Debug)]
pub enum NativeEdgeComposedProcessError {
    Preparation(NativeEdgeProcessError<NativeEdgeRuntimeResourcesError>),
    InvalidBodyDeadline,
    RouteGraph(NativeEdgePublicRouteGraphError),
    PublicListenerInitialization(std::io::Error),
    PrivateListenerInitialization(std::io::Error),
    PublicListener(NativeHttpListenerError),
    PrivateListener(NativeHttpListenerError),
    PostResponseDrain,
}

impl fmt::Display for NativeEdgeComposedProcessError {
    /// Render one bounded process category without resource paths or provider material.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // preserve the owning stage while keeping nested route material out of diagnostics
        match self {
            Self::Preparation(error) => {
                write!(formatter, "native edge process preparation: {error}")
            }
            Self::InvalidBodyDeadline => {
                formatter.write_str("native edge body deadline is invalid")
            }
            Self::RouteGraph(_) => {
                formatter.write_str("native edge public route graph initialization failed")
            }
            Self::PublicListenerInitialization(error) => {
                write!(
                    formatter,
                    "native edge public listener initialization: {error}"
                )
            }
            Self::PrivateListenerInitialization(error) => {
                write!(
                    formatter,
                    "native edge private listener initialization: {error}"
                )
            }
            Self::PublicListener(error) => {
                write!(formatter, "native edge public listener: {error}")
            }
            Self::PrivateListener(error) => {
                write!(formatter, "native edge private listener: {error}")
            }
            Self::PostResponseDrain => {
                formatter.write_str("native edge post-response drain failed")
            }
        }
    }
}

impl std::error::Error for NativeEdgeComposedProcessError {
    /// Preserve only nested errors with already bounded diagnostics.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // route graph errors do not expose a bounded display/source contract
        match self {
            Self::Preparation(error) => Some(error),
            Self::PublicListenerInitialization(error)
            | Self::PrivateListenerInitialization(error) => Some(error),
            Self::PublicListener(error) | Self::PrivateListener(error) => Some(error),
            Self::InvalidBodyDeadline | Self::RouteGraph(_) | Self::PostResponseDrain => None,
        }
    }
}

/// Consume one prepared production process and compose all request owners before socket polling.
pub fn compose_native_edge_process<Codec, Statfs>(
    prepared: NativeEdgePreparedProcess<NativeEdgeRuntimeResources<Codec, Statfs>>,
    public_body_deadline: Duration,
    private_body_deadline: Duration,
) -> Result<NativeEdgeComposedProcess<Codec, Statfs>, NativeEdgeComposedProcessError>
where
    Codec: Clone + Send + 'static + ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: Clone + Send + 'static + weather_edge::ForecastAllocationStatfsPort,
{
    let private_required = prepared.private_local_addr().is_some();
    let parts = prepared
        .into_composition_parts(private_required)
        .map_err(NativeEdgeComposedProcessError::Preparation)?;
    NativeEdgeComposedProcess::from_parts(parts, public_body_deadline, private_body_deadline)
}

impl<Codec, Statfs> NativeEdgeComposedProcess<Codec, Statfs>
where
    Codec: Clone + Send + 'static + ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: Clone + Send + 'static + weather_edge::ForecastAllocationStatfsPort,
{
    /// Consume every initialized startup owner and construct route/post-response owners before polling.
    pub(crate) fn from_parts(
        parts: NativeEdgeCompositionParts<NativeEdgeRuntimeResources<Codec, Statfs>>,
        public_body_deadline: Duration,
        private_body_deadline: Duration,
    ) -> Result<Self, NativeEdgeComposedProcessError> {
        let NativeEdgeCompositionParts {
            resources,
            site,
            admin,
            evidence,
            revision_archive,
            archive,
            scheduler,
            assets,
            public_listener,
            private_listener,
            public_configuration,
            private_configuration,
        } = parts;
        let private_enabled = private_listener.is_some();
        // require finite body ownership only for listeners admitted into this process
        if public_body_deadline.is_zero() || (private_enabled && private_body_deadline.is_zero()) {
            return Err(NativeEdgeComposedProcessError::InvalidBodyDeadline);
        }
        let channel_bound_capture_active = evidence
            .shared()
            .lock()
            .map(|store| store.initialized())
            .unwrap_or(false);
        let (mut startup, client, scheduler_registry) = resources.into_route_inputs();
        let release = startup.configuration.release.clone();
        let admin_admission =
            NativeEdgeAdminAdmissionOwner::from_configuration(&startup.configuration);
        let graph =
            NativeEdgePublicRouteGraph::initialize(&mut startup, admin, assets, site, client)
                .map_err(NativeEdgeComposedProcessError::RouteGraph)?;
        let routes = NativeEdgeRouteExecution::new(
            Arc::new(graph),
            NonZeroUsize::new(PUBLIC_ROUTE_WORKERS).expect("fixed worker count is nonzero"),
        );
        let archive = match archive {
            EdgeStartupArchive::Disabled => NativeEdgePostResponseArchive::Disabled,
            EdgeStartupArchive::Unavailable => NativeEdgePostResponseArchive::Unavailable,
            EdgeStartupArchive::Ready(archive) => NativeEdgePostResponseArchive::Ready(archive),
        };
        let post_response = NativeEdgePostResponseLane::new(evidence, archive);
        let public_handler = NativeEdgePublicHandler::new(
            routes,
            admin_admission,
            post_response,
            scheduler_registry,
            release,
            public_body_deadline,
        );
        Ok(Self {
            public_listener,
            private_listener,
            public_configuration,
            private_configuration,
            public_handler,
            private_handler: private_enabled
                .then(|| NativeEdgePrivateHandler::new(revision_archive, private_body_deadline)),
            scheduler,
            channel_bound_capture_active,
        })
    }

    /// Serve both real listeners, drain scheduler work, then close route and evidence lanes.
    pub async fn serve<Shutdown>(
        self,
        shutdown: Shutdown,
    ) -> Result<NativeEdgeProcessReports, NativeEdgeComposedProcessError>
    where
        Shutdown: Future<Output = ()>,
    {
        let Self {
            public_listener,
            private_listener,
            public_configuration,
            private_configuration,
            public_handler,
            private_handler,
            mut scheduler,
            channel_bound_capture_active,
        } = self;
        let public_listener = match tokio::net::TcpListener::from_std(public_listener) {
            Ok(listener) => listener,
            Err(error) => {
                scheduler.stop_scheduler();
                poll_fn(|context| scheduler.poll_drained(context)).await;
                let _ = public_handler.close_and_drain().await;
                return Err(NativeEdgeComposedProcessError::PublicListenerInitialization(error));
            }
        };
        let private_listener = match private_listener
            .map(tokio::net::TcpListener::from_std)
            .transpose()
        {
            Ok(listener) => listener,
            Err(error) => {
                scheduler.stop_scheduler();
                poll_fn(|context| scheduler.poll_drained(context)).await;
                let _ = public_handler.close_and_drain().await;
                return Err(NativeEdgeComposedProcessError::PrivateListenerInitialization(error));
            }
        };
        let (stop_sender, stop_receiver) = native_http_listener_stop_fanout();
        let public_service_handler = public_handler.clone();
        let public_service =
            move |request: Request<Incoming>, peer, scope: NativeHttpConnectionScope| {
                let handler = public_service_handler.clone();
                async move {
                    handler
                        .serve(request, peer, scope, channel_bound_capture_active)
                        .await
                }
            };
        let mut public_listener = Box::pin(serve_native_edge_http1(
            public_listener,
            public_configuration,
            public_service,
            wait_for_native_http_stop(stop_receiver.clone()),
        ));
        let scheduler = Rc::new(RefCell::new(scheduler));
        let public_scheduler = Rc::clone(&scheduler);
        let scheduler_stop = stop_receiver.clone();
        let public: NativeHttpServeFuture = Box::pin(async move {
            let mut scheduler_active = true;
            let result = poll_fn(|context| {
                // prioritize real socket progress before admitting another scheduler tick
                if let Poll::Ready(result) = public_listener.as_mut().poll(context) {
                    return Poll::Ready(result);
                }
                // stop new scheduler work immediately after coordinated process shutdown
                if scheduler_active && !*scheduler_stop.borrow() {
                    scheduler_active = public_scheduler
                        .borrow_mut()
                        .poll_scheduler(context)
                        .is_pending();
                }
                Poll::Pending
            })
            .await;
            public_scheduler.borrow_mut().stop_scheduler();
            // retain scheduler owners until every already-admitted blocking due operation exits
            poll_fn(|context| public_scheduler.borrow_mut().poll_drained(context)).await;
            result
        });
        let private = private_listener.zip(private_handler.clone()).map(
            |(private_listener, private_handler)| {
                let private_service = move |request: Request<Incoming>, _peer| {
                    let handler = private_handler.clone();
                    async move { handler.serve(request).await }
                };
                Box::pin(serve_native_http1(
                    private_listener,
                    private_configuration,
                    private_service,
                    wait_for_native_http_stop(stop_receiver),
                )) as NativeHttpServeFuture
            },
        );
        let listener_result =
            serve_coordinated_native_http_listeners(stop_sender, public, private, shutdown).await;
        // drain only the private owner that was admitted with a configured private socket
        if let Some(private_handler) = private_handler {
            private_handler.drain().await;
        }
        let drain_result = public_handler.close_and_drain().await;
        let reports = listener_result.map_err(map_listener_error)?;
        // report terminal persistence failure only after both sockets and scheduler have drained
        if drain_result.is_err() {
            return Err(NativeEdgeComposedProcessError::PostResponseDrain);
        }
        Ok(NativeEdgeProcessReports {
            public: reports.primary,
            private: reports.secondary,
        })
    }
}

/// Map the coordinated listener position after both listener futures have terminated.
fn map_listener_error(error: CoordinatedNativeHttpListenerError) -> NativeEdgeComposedProcessError {
    // preserve public/private ownership after the coordinator has reaped both siblings
    match error {
        CoordinatedNativeHttpListenerError::Primary(error) => {
            NativeEdgeComposedProcessError::PublicListener(error)
        }
        CoordinatedNativeHttpListenerError::Secondary(error) => {
            NativeEdgeComposedProcessError::PrivateListener(error)
        }
    }
}
