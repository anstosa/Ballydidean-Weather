//! Installed native edge startup over the admitted process configuration and real resources.

use std::{fmt, future::Future, num::NonZeroU16, sync::Arc, time::Duration};

use weather_edge::{
    EdgeStartupLifecycleConfiguration, ForecastAllocationStatfsPort, ForecastCycleHashPair,
    ForecastEvidenceReceiptCodec,
};

use crate::{
    NativeEdgeComposedProcessError, NativeEdgeConfigurationError, NativeEdgePreparedProcess,
    NativeEdgeProcessError, NativeEdgeProcessReports, NativeEdgeRuntimeResources,
    NativeEdgeRuntimeResourcesError, NativeEdgeStartupPorts, NativeForecastAllocationStatfs,
    NativeHttpListenerConfiguration, NativeOutboundHttp, NativeOutboundHttpError,
    NativeScheduledCaptureRegistry, NativeSchedulerCaptureError, compose_native_edge_process,
    load_native_edge_configuration,
};

/// Explicit listener and request deadlines supplied by the installed process binary.
#[derive(Clone, Copy)]
pub struct NativeInstalledEdgeLimits {
    pub public: NativeHttpListenerConfiguration,
    pub private: NativeHttpListenerConfiguration,
    pub public_body_deadline: Duration,
    pub private_body_deadline: Duration,
}

/// Installed configuration, transport, resource, listener, or serving failure category.
#[derive(Debug)]
pub enum NativeInstalledEdgeProcessError {
    Configuration(NativeEdgeConfigurationError),
    Outbound(NativeOutboundHttpError),
    Resources(NativeSchedulerCaptureError),
    Startup(NativeEdgeProcessError<NativeEdgeRuntimeResourcesError>),
    Serve(NativeEdgeComposedProcessError),
}

impl fmt::Display for NativeInstalledEdgeProcessError {
    /// Render one bounded owning stage without configuration paths or credentials.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // preserve only the stable process stage in installed diagnostics
        match self {
            Self::Configuration(_) => {
                formatter.write_str("native edge configuration admission failed")
            }
            Self::Outbound(error) => write!(formatter, "native edge HTTP client: {error}"),
            Self::Resources(_) => {
                formatter.write_str("native edge scheduler resource initialization failed")
            }
            Self::Startup(error) => write!(formatter, "native edge startup: {error}"),
            Self::Serve(error) => write!(formatter, "native edge serving: {error}"),
        }
    }
}

impl std::error::Error for NativeInstalledEdgeProcessError {
    /// Preserve nested errors whose own diagnostics are already bounded.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // environment and scheduler errors do not expose a bounded source contract
        match self {
            Self::Outbound(error) => Some(error),
            Self::Startup(error) => Some(error),
            Self::Serve(error) => Some(error),
            Self::Configuration(_) | Self::Resources(_) => None,
        }
    }
}

/// Run the installed edge with the actual Linux filesystem-capacity sampler.
pub async fn run_native_installed_edge<Codec, Shutdown>(
    codec: Codec,
    limits: NativeInstalledEdgeLimits,
    shutdown: Shutdown,
) -> Result<NativeEdgeProcessReports, NativeInstalledEdgeProcessError>
where
    Codec: Clone + Send + 'static + ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Shutdown: Future<Output = ()>,
{
    run_native_installed_edge_with_statfs(
        codec,
        NativeForecastAllocationStatfs::new(),
        limits,
        shutdown,
    )
    .await
}

/// Run the installed edge with one explicitly admitted codec and filesystem sampler.
pub async fn run_native_installed_edge_with_statfs<Codec, Statfs, Shutdown>(
    codec: Codec,
    statfs: Statfs,
    limits: NativeInstalledEdgeLimits,
    shutdown: Shutdown,
) -> Result<NativeEdgeProcessReports, NativeInstalledEdgeProcessError>
where
    Codec: Clone + Send + 'static + ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: Clone + Send + 'static + ForecastAllocationStatfsPort,
    Shutdown: Future<Output = ()>,
{
    let startup =
        load_native_edge_configuration().map_err(NativeInstalledEdgeProcessError::Configuration)?;
    let lifecycle = EdgeStartupLifecycleConfiguration::from(&startup.configuration);
    let public_port = NonZeroU16::new(startup.configuration.port)
        .expect("native edge admission rejects a zero public port");
    // retain absence rather than inventing a private listener when the source did not configure it
    let private_port = startup
        .configuration
        .adjustment_maintenance_internal_port
        .map(|port| {
            NonZeroU16::new(port).expect("native edge admission rejects a zero private port")
        });
    let client =
        Arc::new(NativeOutboundHttp::new().map_err(NativeInstalledEdgeProcessError::Outbound)?);
    let scheduler = NativeScheduledCaptureRegistry::default();
    let resources = NativeEdgeRuntimeResources::new(startup, client, codec, statfs, scheduler)
        .map_err(NativeInstalledEdgeProcessError::Resources)?;
    let ports = NativeEdgeStartupPorts::production(resources, public_port, private_port);
    let prepared =
        NativeEdgePreparedProcess::initialize(ports, lifecycle, limits.public, limits.private)
            .map_err(NativeInstalledEdgeProcessError::Startup)?;
    let process = compose_native_edge_process(
        prepared,
        limits.public_body_deadline,
        limits.private_body_deadline,
    )
    .map_err(NativeInstalledEdgeProcessError::Serve)?;
    process
        .serve(shutdown)
        .await
        .map_err(NativeInstalledEdgeProcessError::Serve)
}
