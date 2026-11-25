//! Start the actual installed native API and retain the original private-route admission.

use std::{future::Future, time::Duration};

use crate::{
    NativeApiInstalledOwnerError, NativeApiListenerReports, NativeHttpListenerConfiguration,
    NativePostgresApiServeError, NativePostgresApiStartupError,
    native_installed_api_runtime_loader, prepare_native_postgres_api,
    serve_started_native_postgres_api,
};

/// Explicit approved public/private listener resource and internal relay configuration.
#[derive(Clone, Copy)]
pub struct NativeInstalledApiLimits {
    pub public: NativeHttpListenerConfiguration,
    pub maintenance: NativeHttpListenerConfiguration,
    pub archive_timeout: Duration,
    pub maintenance_body_deadline: Duration,
}

/// Exact installed model, listener preparation, and serving failure categories.
#[derive(Debug)]
pub enum NativeInstalledApiProcessError {
    Models(NativeApiInstalledOwnerError),
    Startup(NativePostgresApiStartupError),
    Serve(NativePostgresApiServeError),
}

/// Run the actual installed native API under an independently admitted process shutdown future.
pub async fn run_native_installed_api(
    limits: NativeInstalledApiLimits,
    shutdown: impl Future<Output = ()>,
) -> Result<NativeApiListenerReports, NativeInstalledApiProcessError> {
    let mut models =
        native_installed_api_runtime_loader().map_err(NativeInstalledApiProcessError::Models)?;
    let started = prepare_native_postgres_api(&mut models, limits.public, limits.maintenance)
        .map_err(NativeInstalledApiProcessError::Startup)?;
    // actual startup owns all public and optional private listener admission
    serve_started_native_postgres_api(
        started,
        limits.archive_timeout,
        limits.maintenance_body_deadline,
        shutdown,
    )
    .await
    .map_err(NativeInstalledApiProcessError::Serve)
}
