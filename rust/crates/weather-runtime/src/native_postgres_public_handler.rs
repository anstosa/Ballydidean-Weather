//! Serve concrete PostgreSQL-backed public requests without blocking Hyper's listener runtime.

use hyper::{Method, Request, Response, body::Incoming};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;
use weather_api::{
    InactiveForecastServiceRuntime, WeatherServiceBoundaries, WeatherServiceRainFallbackDiagnostic,
};
use weather_database::ProcessMigrationArtifactFilesystem;

use crate::{
    ApiProcessMigrationSource, FilesystemWeatherServiceExternal, NativeApiTimeValues,
    NativeHttpBody, NativePostgresPool, NativePublicApiHttpRequest, NativePublicApiIngressError,
    NativeRuntimeMaterial, NativeWeatherServiceWireEncoder, NativeWirePrimitives,
    PreparedPublicWeatherApi, WeatherApiProcessClock, system_worker_health_milliseconds,
};

const PUBLIC_REQUEST_WORKERS: usize = 10;

/// Concrete native external values for PostgreSQL public API repositories.
pub type NativePostgresApiExternal = FilesystemWeatherServiceExternal<
    ApiProcessMigrationSource,
    ProcessMigrationArtifactFilesystem,
    NativeRuntimeMaterial,
    NativeRuntimeMaterial,
    NativeWirePrimitives,
    NativeRuntimeMaterial,
>;

/// Bind the original public repository to the native migration digest, time and canary state owner.
pub fn native_postgres_api_external(
    source: ApiProcessMigrationSource,
) -> NativePostgresApiExternal {
    FilesystemWeatherServiceExternal::current_process(
        source,
        NativeRuntimeMaterial,
        NativeRuntimeMaterial,
        NativeWirePrimitives,
        NativeRuntimeMaterial,
    )
}

/// The actual prepared native public app bound to the admitted process pool and immutable models.
pub type NativePostgresPublicApi =
    PreparedPublicWeatherApi<NativePostgresPool, NativePostgresApiExternal>;

/// One request worker retains its existing application, encoder buffer and process clock.
struct NativePublicRequestWorker {
    application: NativePostgresPublicApi,
    encoder: NativeWeatherServiceWireEncoder<NativeWirePrimitives>,
    clock: WeatherApiProcessClock<NativeWirePrimitives>,
    inactive: InactiveForecastServiceRuntime,
}

impl NativePublicRequestWorker {
    /// Clone only shared process resources and prepare one independently reusable request worker.
    fn new(application: &NativePostgresPublicApi, source: ApiProcessMigrationSource) -> Self {
        let inactive = InactiveForecastServiceRuntime::from_startup(application.snapshots());
        Self {
            application: application.worker_with_values(native_postgres_api_external(source)),
            encoder: NativeWeatherServiceWireEncoder::new(NativeWirePrimitives),
            clock: WeatherApiProcessClock::new(
                NativeWirePrimitives,
                system_worker_health_milliseconds,
            ),
            inactive,
        }
    }

    /// Serve one decoded request without sharing request-local serializer or repository state.
    fn serve(
        &mut self,
        request: NativePublicApiHttpRequest,
        time: &NativeApiTimeValues,
    ) -> Result<Response<NativeHttpBody>, hyper::http::Error> {
        let boundaries = WeatherServiceBoundaries {
            nominal_data_at: None,
            time_resolver: &NativeWirePrimitives,
            history: time,
            tides: time,
            forecast: time,
            forecast_runtime: &self.inactive,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: time,
            daily_precipitation: time,
            rain_collection: time,
            forecast_observation: time,
            version: None,
        };
        request.serve_with_rain_diagnostics(
            &mut self.application,
            &mut self.clock,
            &boundaries,
            &mut self.encoder,
            &mut report_rain_diagnostic,
        )
    }
}

/// Record the existing bounded public rain fallback diagnostic without emitting provider content.
fn report_rain_diagnostic(diagnostic: WeatherServiceRainFallbackDiagnostic) -> Result<(), ()> {
    let line = serde_json::json!({ "event": diagnostic.event, "errorCode": diagnostic.error_code, "errorName": diagnostic.error_name, "method": diagnostic.method, "operation": diagnostic.operation, "status": diagnostic.status });
    use std::io::Write as _;
    writeln!(std::io::stderr().lock(), "{line}").map_err(|_| ())
}

/// Reuse bounded native request workers and let Hyper poll unrelated connections during SQL waits.
#[derive(Clone)]
pub struct NativePostgresPublicHandler {
    source: Arc<ApiProcessMigrationSource>,
    time: NativeApiTimeValues,
    permits: Arc<Semaphore>,
    idle: Arc<Mutex<Vec<NativePublicRequestWorker>>>,
}

impl NativePostgresPublicHandler {
    /// Create lazy request lanes bounded by the same maximum as the actual process database pool.
    pub fn new(source: ApiProcessMigrationSource) -> Self {
        Self::with_time_values(source, NativeApiTimeValues::default())
    }

    /// Retain the calendar cache already shared by installed model admission.
    pub fn with_time_values(source: ApiProcessMigrationSource, time: NativeApiTimeValues) -> Self {
        Self {
            source: Arc::new(source),
            time,
            permits: Arc::new(Semaphore::new(PUBLIC_REQUEST_WORKERS)),
            idle: Arc::new(Mutex::new(Vec::with_capacity(PUBLIC_REQUEST_WORKERS))),
        }
    }

    /// Serve the real public API request using an independent worker over the same retained pool.
    pub async fn serve(
        &self,
        request: Request<Incoming>,
        application: Rc<RefCell<NativePostgresPublicApi>>,
    ) -> Result<Response<NativeHttpBody>, hyper::http::Error> {
        let head = request.method() == Method::HEAD;
        let request = match NativePublicApiHttpRequest::read(request, &NativeWirePrimitives) {
            Ok(request) => request,
            Err(error) => return error.response(head),
        };
        // wait asynchronously for a bounded lane instead of blocking the accept/connection driver
        let permit = match Arc::clone(&self.permits).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return NativePublicApiIngressError.response(head),
        };
        let mut worker = match self.idle.lock() {
            Ok(mut idle) => idle.pop().unwrap_or_else(|| {
                NativePublicRequestWorker::new(&application.borrow(), self.source.as_ref().clone())
            }),
            Err(_) => return NativePublicApiIngressError.response(head),
        };
        let idle = Arc::clone(&self.idle);
        let time = self.time.clone();
        // keep synchronous repository ports off the single Hyper reactor and reuse their worker afterward
        let result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let response = worker.serve(request, &time);
            // return the lane even after an ordinary repository or encoding failure
            if let Ok(mut idle) = idle.lock() {
                idle.push(worker);
            }
            response
        })
        .await;
        result.unwrap_or_else(|_| NativePublicApiIngressError.response(head))
    }
    /// Wait until every admitted blocking request returns its retained work permit.
    pub async fn drain(&self) {
        let _ = self
            .permits
            .acquire_many(PUBLIC_REQUEST_WORKERS as u32)
            .await;
    }
}
