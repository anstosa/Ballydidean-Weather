//! Concrete edge startup resources over explicitly admitted evidence filesystem owners.

use std::{fmt, io, sync::Arc};

use hyper::{Method, header::CONTENT_TYPE};
use url::Url;
use weather_edge::{
    BoundedForecastSchedulerSlotReader, ForecastAllocationStatfsPort, ForecastCycleHashPair,
    ForecastEvidenceMaterial, ForecastEvidenceReceiptCodec, ForecastMapSite,
    ForecastSchedulerCaptureDriver, ForecastSchedulerConfiguration,
    ForecastSchedulerPublicReadyStarter, NativeForecastCaptureStore, PrivateArchiveFixedGaugeStore,
    PrivateArchivePermanentGapStore, PrivateArchiveRainControlStore, PrivateArchiveRevisionStore,
    PrivateArchiveShadowStore, PrivateArchiveStorage, PublicApiBodyReadOutcome,
    PublicApiHttpMethod, PublicApiHttpPort, PublicApiHttpRequest, PublicApiHttpResponse,
    PublicApiHttpResponseHead, PublicApiRedirectPolicy,
};

use crate::{
    NativeBackgroundForecastScheduler, NativeEdgeAdminOwner, NativeEdgeAdminOwnerError,
    NativeEdgeConfiguration, NativeEdgeOnlineArchive, NativeEdgePublicAssets,
    NativeEdgePublicAssetsError, NativeEdgeStartupResources, NativeForecastSchedulerTimerError,
    NativeModelEvidenceValues, NativeOutboundHttp, NativeOutboundHttpError,
    NativeOutboundHttpRequest, NativeOutboundHttpResponse, NativeOwnedForecastSchedulerPorts,
    NativeOwnedForecastSchedulerTarget, NativeProcessClock, NativeScheduledCaptureRegistry,
    NativeSchedulerCaptureAdmission, NativeSchedulerCaptureError, NativeSchedulerCaptureHttp,
    NativeSharedForecastResponseEvidenceStore, NativeWirePrimitives,
    bind_native_private_archive_stores, initialize_native_edge_public_assets, native_edge_map_site,
};

const SCHEDULE_RESPONSE_MAXIMUM_BYTES: u64 = 8 * 1024;

/// Shared fail-open native evidence owner selected during the source startup evidence phase.
pub type NativeEdgeRuntimeEvidence<Codec, Statfs> =
    NativeSharedForecastResponseEvidenceStore<NativeForecastCaptureStore<Codec, Statfs>>;

/// Optional online archive owner over the same admitted codec, root, clock, and statfs port.
pub type NativeEdgeRuntimeOnlineArchive<Codec, Statfs> =
    NativeEdgeOnlineArchive<Codec, NativeWirePrimitives, NativeProcessClock, Statfs>;

/// Exact schedule-slot reader over the configured compose-private API origin.
pub type NativeEdgeRuntimeSchedulerSlots<Codec> = BoundedForecastSchedulerSlotReader<
    NativeEdgeSchedulerSlotsHttp,
    ForecastEvidenceMaterial<Codec>,
>;

/// Genuine loopback scheduler trigger sharing the public request association registry.
pub type NativeEdgeRuntimeSchedulerTrigger =
    ForecastSchedulerCaptureDriver<NativeSchedulerCaptureAdmission, NativeSchedulerCaptureHttp>;

/// Owned scheduler target retaining every concrete port for the background due lane.
pub type NativeEdgeRuntimeSchedulerTarget<Codec, Statfs> = NativeOwnedForecastSchedulerTarget<
    NativeWirePrimitives,
    NativeWirePrimitives,
    NativeProcessClock,
    NativeEdgeRuntimeSchedulerSlots<Codec>,
    Statfs,
    NativeEdgeRuntimeSchedulerTrigger,
>;

/// Production scheduler lane started only by the public listener readiness callback.
pub type NativeEdgeRuntimeScheduler<Codec, Statfs> =
    NativeBackgroundForecastScheduler<NativeEdgeRuntimeSchedulerTarget<Codec, Statfs>>;

/// Type-erased ownership of the five real private archive stores.
pub struct NativeEdgeRevisionArchive {
    storage: Box<dyn PrivateArchiveStorage + Send>,
}

impl NativeEdgeRevisionArchive {
    /// Retain the actual composed stores behind their already object-safe listener port.
    fn new(storage: impl PrivateArchiveStorage + Send + 'static) -> Self {
        Self {
            storage: Box::new(storage),
        }
    }
}

impl PrivateArchiveStorage for NativeEdgeRevisionArchive {
    /// Borrow the real rain-control archive store.
    fn rain_control_store(&mut self) -> &mut dyn PrivateArchiveRainControlStore {
        self.storage.rain_control_store()
    }

    /// Borrow the real fixed-gauge archive store.
    fn fixed_gauge_gap_store(&mut self) -> &mut dyn PrivateArchiveFixedGaugeStore {
        self.storage.fixed_gauge_gap_store()
    }

    /// Borrow the real shadow archive store.
    fn shadow_store(&mut self) -> &mut dyn PrivateArchiveShadowStore {
        self.storage.shadow_store()
    }

    /// Borrow the real revision archive store.
    fn revision_store(&mut self) -> &mut dyn PrivateArchiveRevisionStore {
        self.storage.revision_store()
    }

    /// Borrow the real permanent-gap archive store.
    fn permanent_gap_store(&mut self) -> &mut dyn PrivateArchivePermanentGapStore {
        self.storage.permanent_gap_store()
    }
}

/// Actual bounded HTTP port for the scheduler's private schedule-slot read.
#[derive(Clone)]
pub struct NativeEdgeSchedulerSlotsHttp {
    client: Arc<NativeOutboundHttp>,
}

impl NativeEdgeSchedulerSlotsHttp {
    /// Reuse the edge process HTTP pool without starting another runtime.
    pub const fn new(client: Arc<NativeOutboundHttp>) -> Self {
        Self { client }
    }
}

/// One completely consumed bounded schedule response.
pub struct NativeEdgeSchedulerSlotsResponse(NativeOutboundHttpResponse);

impl PublicApiHttpResponse for NativeEdgeSchedulerSlotsResponse {
    type Error = NativeOutboundHttpError;

    /// Expose the received status and bounded representation metadata.
    fn head(&self) -> PublicApiHttpResponseHead {
        PublicApiHttpResponseHead {
            content_length: self
                .0
                .declared_body_bytes
                .and_then(|length| usize::try_from(length).ok()),
            content_type: self
                .0
                .headers
                .get(CONTENT_TYPE)
                .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned()),
            status_code: self.0.status,
        }
    }

    /// Drain the retained response through the scheduler reader's independent eight-KiB ceiling.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<PublicApiBodyReadOutcome, Self::Error> {
        let body = std::mem::take(&mut self.0.body);
        // retain receiver cancellation even though the native transport already bounded the body
        for chunk in body.chunks(8 * 1024) {
            if !receiver(chunk) {
                return Ok(PublicApiBodyReadOutcome::ReceiverStopped);
            }
        }
        Ok(PublicApiBodyReadOutcome::Complete)
    }
}

impl PublicApiHttpPort for NativeEdgeSchedulerSlotsHttp {
    type Error = NativeOutboundHttpError;
    type Response = NativeEdgeSchedulerSlotsResponse;

    /// Execute only the admitted private schedule-slots request through the shared HTTP runtime.
    fn execute(&self, request: &PublicApiHttpRequest) -> Result<Self::Response, Self::Error> {
        // keep this port closed to every route except the scheduler's exact source request
        if request.method != PublicApiHttpMethod::Get
            || request.redirect != PublicApiRedirectPolicy::Manual
            || request.accept != "application/json"
            || request.pathname != "/internal/adjustment-maintenance/schedule-slots"
            || !request.search.is_empty()
        {
            return Err(NativeOutboundHttpError::InvalidRequest);
        }
        let base = Url::parse(request.origin.as_str())
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        // retain the already-admitted credential-free root-origin shape at the transport boundary
        if base.scheme() != "http"
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.path() != "/"
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(NativeOutboundHttpError::InvalidRequest);
        }
        let target = base
            .join(&request.pathname)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        // reject resolution that escapes the admitted compose-private origin
        if target.origin() != base.origin() {
            return Err(NativeOutboundHttpError::InvalidRequest);
        }
        let outbound = NativeOutboundHttpRequest::new(Method::GET, target)?
            .manual_redirect()
            .stream_body_limit_only();
        self.client
            .execute(
                outbound,
                std::time::Duration::from_millis(request.timeout_ms),
                SCHEDULE_RESPONSE_MAXIMUM_BYTES,
            )
            .map(NativeEdgeSchedulerSlotsResponse)
    }
}

/// Concrete startup resource failures while preserving optional evidence/archive containment.
#[derive(Debug)]
pub enum NativeEdgeRuntimeResourcesError {
    Admin(NativeEdgeAdminOwnerError),
    RevisionArchive(io::Error),
    OnlineArchive(io::Error),
    Assets(NativeEdgePublicAssetsError),
    Scheduler(NativeForecastSchedulerTimerError),
}

impl fmt::Display for NativeEdgeRuntimeResourcesError {
    /// Render only bounded startup categories without configured paths or provider material.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // retain source categories while avoiding asset or filesystem path disclosure
        match self {
            Self::Admin(error) => write!(formatter, "edge administrator initialization: {error}"),
            Self::RevisionArchive(_) => {
                formatter.write_str("edge revision archive initialization failed")
            }
            Self::OnlineArchive(_) => {
                formatter.write_str("edge online archive initialization failed")
            }
            Self::Assets(_) => formatter.write_str("edge public asset initialization failed"),
            Self::Scheduler(error) => write!(formatter, "edge scheduler start: {error}"),
        }
    }
}

impl std::error::Error for NativeEdgeRuntimeResourcesError {
    /// Preserve only nested errors whose diagnostics are already bounded.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // asset errors intentionally retain no path-bearing nested source here
        match self {
            Self::Admin(error) => Some(error),
            Self::RevisionArchive(error) | Self::OnlineArchive(error) => Some(error),
            Self::Scheduler(error) => Some(error),
            Self::Assets(_) => None,
        }
    }
}

/// One edge startup owner retaining configuration and explicitly admitted storage primitives.
pub struct NativeEdgeRuntimeResources<Codec, Statfs> {
    startup: NativeEdgeConfiguration,
    client: Arc<NativeOutboundHttp>,
    codec: Codec,
    statfs: Statfs,
    site: Option<ForecastMapSite>,
    scheduler_admission: Option<NativeSchedulerCaptureAdmission>,
    scheduler_registry: NativeScheduledCaptureRegistry,
}

impl<Codec, Statfs> NativeEdgeRuntimeResources<Codec, Statfs> {
    /// Bind process owners without touching evidence, archive, admin, or asset storage.
    pub fn new(
        startup: NativeEdgeConfiguration,
        client: Arc<NativeOutboundHttp>,
        codec: Codec,
        statfs: Statfs,
        scheduler_registry: NativeScheduledCaptureRegistry,
    ) -> Result<Self, NativeSchedulerCaptureError> {
        let scheduler_admission = NativeSchedulerCaptureAdmission::new(
            startup.configuration.port,
            scheduler_registry.clone(),
        )?;
        Ok(Self {
            startup,
            client,
            codec,
            statfs,
            site: None,
            scheduler_admission: Some(scheduler_admission),
            scheduler_registry,
        })
    }

    /// Borrow the same scheduler association registry installed in the public handler.
    pub fn scheduler_registry(&self) -> NativeScheduledCaptureRegistry {
        self.scheduler_registry.clone()
    }

    /// Borrow the process HTTP runtime retained for public route-graph transfer.
    pub fn client(&self) -> Arc<NativeOutboundHttp> {
        Arc::clone(&self.client)
    }

    /// Borrow startup configuration before route-graph transfer consumes map credentials.
    pub const fn startup_mut(&mut self) -> &mut NativeEdgeConfiguration {
        &mut self.startup
    }

    /// Release route-only configuration, client, and registry after lifecycle owners are extracted.
    pub fn into_route_inputs(
        self,
    ) -> (
        NativeEdgeConfiguration,
        Arc<NativeOutboundHttp>,
        NativeScheduledCaptureRegistry,
    ) {
        (self.startup, self.client, self.scheduler_registry)
    }
}

impl<Codec, Statfs> NativeEdgeStartupResources for NativeEdgeRuntimeResources<Codec, Statfs>
where
    Codec: Clone + Send + 'static + ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: Clone + Send + 'static + ForecastAllocationStatfsPort,
{
    type Site = ForecastMapSite;
    type Admin = NativeEdgeAdminOwner;
    type Evidence = NativeEdgeRuntimeEvidence<Codec, Statfs>;
    type RevisionArchive = NativeEdgeRevisionArchive;
    type Archive = NativeEdgeRuntimeOnlineArchive<Codec, Statfs>;
    type Scheduler = NativeEdgeRuntimeScheduler<Codec, Statfs>;
    type Assets = NativeEdgePublicAssets;
    type Error = NativeEdgeRuntimeResourcesError;

    /// Load optional map metadata first and retain the same value for later asset initialization.
    fn load_site(&mut self) -> Option<Self::Site> {
        self.site = native_edge_map_site(&self.startup.configuration);
        self.site
    }

    /// Construct the actual administrator owner after optional site admission.
    fn initialize_admin(&mut self, site: Option<&Self::Site>) -> Result<Self::Admin, Self::Error> {
        NativeEdgeAdminOwner::from_configuration(&self.startup.configuration, site)
            .map_err(NativeEdgeRuntimeResourcesError::Admin)
    }

    /// Initialize the real evidence store while retaining its built-in fail-open status.
    fn initialize_evidence(&mut self) -> Self::Evidence {
        let store = NativeForecastCaptureStore::initialize(
            self.startup.configuration.adjustment_evidence_root.clone(),
            self.codec.clone(),
            self.statfs.clone(),
            &mut NativeProcessClock,
        );
        NativeSharedForecastResponseEvidenceStore::new(store)
    }

    /// Bind all five native private archive stores to the admitted evidence root.
    fn initialize_revision_archive(&mut self) -> Result<Self::RevisionArchive, Self::Error> {
        bind_native_private_archive_stores(
            self.startup.configuration.adjustment_evidence_root.clone(),
            NativeModelEvidenceValues::new(),
            NativeProcessClock,
        )
        .map(NativeEdgeRevisionArchive::new)
        .map_err(NativeEdgeRuntimeResourcesError::RevisionArchive)
    }

    /// Initialize the genuinely enabled online archive with the admitted codec and statfs owner.
    fn initialize_archive(&mut self) -> Result<Self::Archive, Self::Error> {
        NativeEdgeOnlineArchive::initialize(
            self.startup.configuration.adjustment_evidence_root.clone(),
            ForecastEvidenceMaterial::new(self.codec.clone()),
            NativeWirePrimitives,
            NativeProcessClock,
            self.statfs.clone(),
        )
        .map_err(NativeEdgeRuntimeResourcesError::OnlineArchive)
    }

    /// Recover the real scheduler and retain its due work off the listener reactor.
    fn initialize_scheduler(
        &mut self,
        configuration: ForecastSchedulerConfiguration,
    ) -> Self::Scheduler {
        let admission = self
            .scheduler_admission
            .take()
            .expect("edge lifecycle initializes the scheduler exactly once");
        let slots = BoundedForecastSchedulerSlotReader::new(
            self.startup
                .configuration
                .adjustment_maintenance_api_origin
                .clone(),
            NativeEdgeSchedulerSlotsHttp::new(Arc::clone(&self.client)),
            ForecastEvidenceMaterial::new(self.codec.clone()),
        );
        let trigger = ForecastSchedulerCaptureDriver::new(
            admission,
            NativeSchedulerCaptureHttp::new(Arc::clone(&self.client)),
        );
        let target = NativeOwnedForecastSchedulerTarget::initialize(
            self.startup.configuration.adjustment_evidence_root.clone(),
            configuration,
            NativeOwnedForecastSchedulerPorts {
                codec: NativeWirePrimitives,
                calendar: NativeWirePrimitives,
                clock: NativeProcessClock,
                slots,
                filesystem: self.statfs.clone(),
                trigger,
            },
        );
        NativeBackgroundForecastScheduler::new(target)
    }

    /// Load the fixed release assets after scheduler recovery in predecessor startup order.
    fn load_assets(&mut self) -> Result<Self::Assets, Self::Error> {
        initialize_native_edge_public_assets(&self.startup.configuration, self.site)
            .map_err(NativeEdgeRuntimeResourcesError::Assets)
    }

    /// Start only the already initialized scheduler from public listener readiness.
    fn start_scheduler(&mut self, scheduler: &mut Self::Scheduler) -> Result<bool, Self::Error> {
        scheduler
            .start_on_public_ready()
            .map_err(NativeEdgeRuntimeResourcesError::Scheduler)
    }
}
