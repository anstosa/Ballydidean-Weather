use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Condvar, Mutex};

const CACHE_TTL_MS: i64 = 5 * 60 * 1_000;

/// one complete queryless forecast-observation response admitted by its encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastObservationsJson(Arc<[u8]>);

impl AdmittedForecastObservationsJson {
    /// share one complete compact recorded-observation response across requests.
    pub fn admitted(content: impl Into<Vec<u8>>) -> Self {
        Self(content.into().into())
    }

    /// borrow the complete admitted response bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// one complete queryless solar-cloud-evidence response admitted by its encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedSolarCloudEvidenceJson(Arc<[u8]>);

impl AdmittedSolarCloudEvidenceJson {
    /// share one complete compact recorded-evidence response across requests.
    pub fn admitted(content: impl Into<Vec<u8>>) -> Self {
        Self(content.into().into())
    }

    /// borrow the complete admitted response bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// one complete bounded archive fill with separate recorded products.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastRecordedProducts {
    pub day: String,
    pub forecast_observations: AdmittedForecastObservationsJson,
    pub generated_at: String,
    pub solar_cloud_evidence: AdmittedSolarCloudEvidenceJson,
}

/// one admitted cache observation without a calendar codec here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastRecordedNow {
    pub epoch_ms: i64,
    pub local_day: String,
}

/// current epoch and farm-local day observation.
pub trait ForecastRecordedProductsClock {
    /// return the current admitted cache instant and local day.
    fn now(&self) -> AdmittedForecastRecordedNow;
}

/// bounded archive loading retained outside response and cache policy.
pub trait ForecastRecordedProductsLoader {
    type Error;

    /// build one complete admitted projection from a bounded archive fill.
    fn fill(
        &self,
        request_started_at_epoch_ms: i64,
    ) -> Result<AdmittedForecastRecordedProducts, Self::Error>;
}

/// one archive fill or retry-cooldown refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastRecordedProductsCacheError<LoaderError> {
    Loader(LoaderError),
    RetryCooldown,
}

impl<LoaderError: Display> Display for ForecastRecordedProductsCacheError<LoaderError> {
    /// render the source cache's current failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the archive error or exact cooldown text
        match self {
            Self::Loader(error) => Display::fmt(error, formatter),
            Self::RetryCooldown => {
                formatter.write_str("Forecast observation retry cooldown is active")
            }
        }
    }
}

impl<LoaderError> Error for ForecastRecordedProductsCacheError<LoaderError>
where
    LoaderError: Error + 'static,
{
    /// retain only an underlying archive fill source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose no invented source for cooldown refusal
        match self {
            Self::Loader(error) => Some(error),
            Self::RetryCooldown => None,
        }
    }
}

#[derive(Debug, Clone)]
struct CachedProducts {
    body: AdmittedForecastRecordedProducts,
    expires_at_epoch_ms: i64,
}

#[derive(Debug)]
struct PendingFill<LoaderError> {
    completed: Mutex<Option<Result<(), ForecastRecordedProductsCacheError<LoaderError>>>>,
    ready: Condvar,
}

impl<LoaderError: Clone> PendingFill<LoaderError> {
    /// create one unresolved shared archive fill.
    fn new() -> Self {
        Self {
            completed: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    /// wake pending readers with the fill status rather than a duplicate response.
    fn complete(&self, result: Result<(), ForecastRecordedProductsCacheError<LoaderError>>) {
        let mut completed = self
            .completed
            .lock()
            .expect("forecast recorded pending lock poisoned");
        *completed = Some(result);
        self.ready.notify_all();
    }

    /// wait for the shared fill status before rereading the current cache.
    fn wait(&self) -> Result<(), ForecastRecordedProductsCacheError<LoaderError>> {
        let mut completed = self
            .completed
            .lock()
            .expect("forecast recorded pending lock poisoned");

        // wait only while the archive fill remains unresolved
        while completed.is_none() {
            completed = self
                .ready
                .wait(completed)
                .expect("forecast recorded pending lock poisoned");
        }

        completed
            .as_ref()
            .expect("completed forecast recorded fill")
            .clone()
    }
}

#[derive(Debug)]
struct ForecastRecordedProductsCacheState<LoaderError> {
    cached: Option<CachedProducts>,
    pending: Option<Arc<PendingFill<LoaderError>>>,
    retry_after_epoch_ms: i64,
}

/// one shared same-local-day recorded-products cache.
#[derive(Debug)]
pub struct ForecastRecordedProductsCache<Clock, Loader>
where
    Loader: ForecastRecordedProductsLoader,
{
    clock: Clock,
    loader: Loader,
    retry_ms: i64,
    state: Mutex<ForecastRecordedProductsCacheState<Loader::Error>>,
}

impl<Clock, Loader> ForecastRecordedProductsCache<Clock, Loader>
where
    Clock: ForecastRecordedProductsClock,
    Loader: ForecastRecordedProductsLoader,
    Loader::Error: Clone,
{
    /// configure one empty shared archive cache.
    pub fn new(clock: Clock, loader: Loader, retry_ms: i64) -> Self {
        Self {
            clock,
            loader,
            retry_ms,
            state: Mutex::new(ForecastRecordedProductsCacheState {
                cached: None,
                pending: None,
                retry_after_epoch_ms: 0,
            }),
        }
    }

    /// read one same-day completed projection or coordinate one bounded fill.
    pub fn get(
        &self,
    ) -> Result<AdmittedForecastRecordedProducts, ForecastRecordedProductsCacheError<Loader::Error>>
    {
        let now = self.clock.now();
        let pending = {
            let mut state = self
                .state
                .lock()
                .expect("forecast recorded cache lock poisoned");

            // retain only a fresh projection from the current farm day
            if let Some(cached) = &state.cached
                && cached.body.day == now.local_day
                && cached.expires_at_epoch_ms > now.epoch_ms
            {
                return Ok(cached.body.clone());
            }

            // share one current fill before applying retry cooldown
            if let Some(pending) = &state.pending {
                Some(Arc::clone(pending))
            } else {
                // reject repeated public retry pressure during a failed-fill cooldown
                if state.retry_after_epoch_ms > now.epoch_ms {
                    return Err(ForecastRecordedProductsCacheError::RetryCooldown);
                }
                state.pending = Some(Arc::new(PendingFill::new()));
                None
            }
        };

        // wait on one already-running fill and recheck day and expiry after success
        if let Some(pending) = pending {
            pending.wait()?;
            return self.get();
        }

        let loaded = self
            .loader
            .fill(now.epoch_ms)
            .map_err(ForecastRecordedProductsCacheError::Loader);
        let result = {
            let mut state = self
                .state
                .lock()
                .expect("forecast recorded cache lock poisoned");

            // publish only one complete successful fill
            let outcome = match loaded {
                Ok(body) => {
                    state.cached = Some(CachedProducts {
                        body,
                        expires_at_epoch_ms: now.epoch_ms.saturating_add(CACHE_TTL_MS),
                    });
                    Ok(())
                }
                Err(error) => {
                    state.retry_after_epoch_ms =
                        self.clock.now().epoch_ms.saturating_add(self.retry_ms);
                    Err(error)
                }
            };
            let pending = state.pending.take().expect("owned forecast recorded fill");
            pending.complete(outcome.clone());
            outcome
        };
        result?;
        self.get()
    }
}

/// the two independent public recorded-product routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastRecordedProductRoute {
    ForecastObservations,
    SolarCloudEvidence,
}

/// one decoded read-only route method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastRecordedProductMethod {
    Get,
    Head,
    Unsupported,
}

/// one queryless route request after URL parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastRecordedProductRequest {
    pub has_query: bool,
    pub method: ForecastRecordedProductMethod,
    pub route: ForecastRecordedProductRoute,
}

/// one response body retained for the owning protocol adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastRecordedProductBody {
    Text(&'static [u8]),
    Json(Option<Arc<[u8]>>),
}

/// exact ordinary response headers for the recorded routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastRecordedProductHeaders {
    pub allow: Option<&'static str>,
    pub cache_control: &'static str,
    pub content_length: usize,
    pub content_type: &'static str,
    pub retry_after: Option<&'static str>,
}

/// one typed recorded-product route response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastRecordedProductResponse {
    pub body: ForecastRecordedProductBody,
    pub headers: ForecastRecordedProductHeaders,
    pub status_code: u16,
}

/// recorded-products lookup retained outside route response policy.
pub trait ForecastRecordedProductsRead {
    type Error;

    /// read one complete shared recorded-products projection.
    fn get_recorded_products(&self) -> Result<AdmittedForecastRecordedProducts, Self::Error>;
}

impl<Clock, Loader> ForecastRecordedProductsRead for ForecastRecordedProductsCache<Clock, Loader>
where
    Clock: ForecastRecordedProductsClock,
    Loader: ForecastRecordedProductsLoader,
    Loader::Error: Clone,
{
    type Error = ForecastRecordedProductsCacheError<Loader::Error>;

    /// delegate to the shared same-day cache coordinator.
    fn get_recorded_products(&self) -> Result<AdmittedForecastRecordedProducts, Self::Error> {
        self.get()
    }
}

/// execute one queryless recorded-product request over the shared cache.
pub fn handle_forecast_recorded_product<Store>(
    request: ForecastRecordedProductRequest,
    store: &Store,
) -> ForecastRecordedProductResponse
where
    Store: ForecastRecordedProductsRead,
{
    // reject mutation methods before query or archive work
    if request.method == ForecastRecordedProductMethod::Unsupported {
        return recorded_text_response(405, b"method not allowed\n", Some("GET, HEAD"), None);
    }

    // keep both recorded routes queryless and bounded
    if request.has_query {
        return recorded_text_response(400, b"unsupported query\n", None, None);
    }
    let products = match store.get_recorded_products() {
        Ok(products) => products,
        Err(_) => {
            return recorded_text_response(
                503,
                b"recorded history is temporarily unavailable\n",
                None,
                Some("30"),
            );
        }
    };
    let content = match request.route {
        ForecastRecordedProductRoute::ForecastObservations => products.forecast_observations.0,
        ForecastRecordedProductRoute::SolarCloudEvidence => products.solar_cloud_evidence.0,
    };
    // retain the original cached bytes for GET and only the representation length for HEAD
    let content_length = content.len();
    let body = (request.method != ForecastRecordedProductMethod::Head).then_some(content);
    ForecastRecordedProductResponse {
        body: ForecastRecordedProductBody::Json(body),
        headers: ForecastRecordedProductHeaders {
            allow: None,
            cache_control: "public, max-age=60, s-maxage=60",
            content_length,
            content_type: "application/json; charset=utf-8",
            retry_after: None,
        },
        status_code: 200,
    }
}

/// build one exact source text response without protocol I/O.
fn recorded_text_response(
    status_code: u16,
    body: &'static [u8],
    allow: Option<&'static str>,
    retry_after: Option<&'static str>,
) -> ForecastRecordedProductResponse {
    ForecastRecordedProductResponse {
        body: ForecastRecordedProductBody::Text(body),
        headers: ForecastRecordedProductHeaders {
            allow,
            cache_control: "no-store",
            content_length: body.len(),
            content_type: "text/plain; charset=utf-8",
            retry_after,
        },
        status_code,
    }
}
