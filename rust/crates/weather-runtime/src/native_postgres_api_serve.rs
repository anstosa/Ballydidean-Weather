//! Run the prepared native API listeners with actual public, private, and drain owners.

use std::{future::Future, sync::Arc, time::Duration};

use crate::{
    NativeApiCaptureExecution, NativeApiCapturePredictionOwner, NativeApiListenerError,
    NativeApiListenerReports, NativeApiRevisionDocumentDecoder, NativeOutboundHttp,
    NativeOutboundHttpError, NativePostgresError, NativePostgresPrivateHandler,
    StartedNativePostgresApi,
};

/// Separate socket failures, outbound startup, and retained work shutdown categories.
#[derive(Debug)]
pub enum NativePostgresApiServeError {
    Listener(NativeApiListenerError),
    Outbound(NativeOutboundHttpError),
    CaptureDrain,
    DatabaseDrain(NativePostgresError),
}

/// Serve every startup-admitted native API route under the same shutdown ownership.
///
/// No maintenance socket or archive client is created when the original startup omitted it.
pub async fn serve_started_native_postgres_api(
    application: StartedNativePostgresApi,
    archive_timeout: Duration,
    private_body_deadline: Duration,
    shutdown: impl Future<Output = ()>,
) -> Result<NativeApiListenerReports, NativePostgresApiServeError> {
    let StartedNativePostgresApi {
        started,
        listeners,
        public_handler,
        models,
    } = application;
    let pool = listeners
        .public_pool()
        .ok_or(NativePostgresApiServeError::Listener(
            NativeApiListenerError::MissingPublicListener,
        ))?;
    let public = public_handler.clone();
    let public_dispatch = move |request, _peer, application| {
        let handler = public.clone();
        async move { handler.serve(request, application).await }
    };
    // capture work and its pooled relay only exist after the original witness and origin admission
    let maintenance = if started.maintenance_listener_active {
        let (pool, origin) =
            listeners
                .maintenance_runtime()
                .ok_or(NativePostgresApiServeError::Listener(
                    NativeApiListenerError::MissingMaintenanceListener,
                ))?;
        let client =
            Arc::new(NativeOutboundHttp::new().map_err(NativePostgresApiServeError::Outbound)?);
        let prediction = NativeApiCapturePredictionOwner::new(
            pool,
            origin.clone(),
            Arc::clone(&client),
            archive_timeout,
        );
        let capture = NativeApiCaptureExecution::new(prediction);
        let handler = NativePostgresPrivateHandler::new(
            origin,
            client,
            archive_timeout,
            private_body_deadline,
            models.as_ref().clone(),
            capture.clone(),
            NativeApiRevisionDocumentDecoder,
        );
        Some((handler, capture))
    } else {
        None
    };
    let listener_result = if let Some((handler, _capture)) = &maintenance {
        let handler = handler.clone();
        let private_dispatch = move |request, _peer, application| {
            let handler = handler.clone();
            async move { handler.serve(request, application).await }
        };
        listeners
            .serve_with_maintenance(public_dispatch, private_dispatch, shutdown)
            .await
    } else {
        listeners.serve_public(public_dispatch, shutdown).await
    };
    // drain the retained lanes even when a listener reported an error after partial serving
    public_handler.drain().await;
    let capture_drain = if let Some((handler, capture)) = maintenance {
        handler.drain().await;
        tokio::task::spawn_blocking(move || capture.drain())
            .await
            .map_err(|_| NativePostgresApiServeError::CaptureDrain)
            .and_then(|value| value.map_err(|_| NativePostgresApiServeError::CaptureDrain))
    } else {
        Ok(())
    };
    let database_drain = tokio::task::spawn_blocking(move || pool.close())
        .await
        .map_err(|_| NativePostgresApiServeError::CaptureDrain)
        .and_then(|value| value.map_err(NativePostgresApiServeError::DatabaseDrain));
    // preserve the original socket failure while still settling every retained owner
    let reports = listener_result.map_err(NativePostgresApiServeError::Listener)?;
    capture_drain?;
    database_drain?;
    Ok(reports)
}
