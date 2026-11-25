//! Pooled native HTTP/1.1 and TLS transport for provider exchanges.

use std::fmt::{self, Display, Formatter};
use std::io::{self, Read};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use http_body_util::{BodyExt as _, Full};
use hyper::body::Bytes;
use hyper::header::{
    ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, COOKIE,
    HeaderMap, HeaderName, HeaderValue, LOCATION,
};
use hyper::{Method, Request, StatusCode, Uri};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use rustls::{ClientConfig, RootCertStore};
use tokio::sync::mpsc as tokio_mpsc;
use url::Url;
use weather_providers::{
    ContentBudgetRole, ProviderAttempt, ProviderAttemptError, ProviderAttemptResponse,
    ProviderError, ProviderFailure, ProviderFailureClassification, ProviderIo, ProviderSleep,
    ProviderSleepError,
};

use crate::system_worker_health_milliseconds;

const DEFAULT_REDIRECT_LIMIT: u8 = 20;
const OUTBOUND_COMMAND_CAPACITY: usize = 16;
const OUTBOUND_CONCURRENCY_LIMIT: usize = 16;
const PROVIDER_RESPONSE_TOO_LARGE: &str = "provider_response_too_large";

type NativeHttpsConnector = HttpsConnector<HttpConnector>;
type NativeHyperClient = Client<NativeHttpsConnector, Full<Bytes>>;

/// A request with source-owned method, headers, body, and redirect policy.
#[derive(Clone, Debug)]
pub struct NativeOutboundHttpRequest {
    body: Bytes,
    headers: HeaderMap,
    method: Method,
    redirect_limit: u8,
    manual_redirect: bool,
    check_declared_length: bool,
    url: Url,
}

impl NativeOutboundHttpRequest {
    /// Admit one absolute HTTP or HTTPS target without embedded credentials.
    pub fn new(method: Method, url: Url) -> Result<Self, NativeOutboundHttpError> {
        validate_url(&url)?;
        Ok(Self {
            body: Bytes::new(),
            headers: HeaderMap::new(),
            method,
            redirect_limit: DEFAULT_REDIRECT_LIMIT,
            manual_redirect: false,
            check_declared_length: true,
            url,
        })
    }

    /// Replace one request header with its already-validated HTTP representation.
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    /// Replace the finite request body without changing method semantics.
    pub fn body(mut self, body: impl Into<Bytes>) -> Self {
        self.body = body.into();
        self
    }

    /// Preserve the response status for callers whose source uses Fetch manual redirects.
    pub fn manual_redirect(mut self) -> Self {
        self.manual_redirect = true;
        self
    }

    /// Use the source edge's bounded streamed body rule without imposing a GET header limit.
    pub fn stream_body_limit_only(mut self) -> Self {
        self.check_declared_length = false;
        self
    }

    /// Override the Fetch-compatible maximum redirect count.
    pub fn redirect_limit(mut self, redirect_limit: u8) -> Self {
        self.redirect_limit = redirect_limit;
        self
    }
}

/// One completely consumed response with transport evidence retained for provider policy.
#[derive(Clone, Debug)]
pub struct NativeOutboundHttpResponse {
    pub body: Vec<u8>,
    pub declared_body_bytes: Option<u64>,
    pub headers: HeaderMap,
    pub status: u16,
    pub streamed_body_bytes: u64,
}

/// Native root-loading or protocol-version failures before a TLS client exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeTlsConfigError {
    NativeRootsUnavailable,
    ProtocolVersionsUnavailable,
}

impl Display for NativeTlsConfigError {
    /// Describe the bounded TLS setup stage without certificate contents.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // retain only stable setup categories
        match self {
            Self::NativeRootsUnavailable => formatter.write_str("native TLS roots are unavailable"),
            Self::ProtocolVersionsUnavailable => {
                formatter.write_str("safe TLS protocol versions are unavailable")
            }
        }
    }
}

impl std::error::Error for NativeTlsConfigError {}

/// Bounded failures from request admission, the retained runtime, or response consumption.
#[derive(Debug)]
pub enum NativeOutboundHttpError {
    BodyDecode,
    BodyTooLarge {
        role: ContentBudgetRole,
        status: u16,
    },
    Deadline,
    InvalidRedirect,
    InvalidRequest,
    QueueSaturated,
    RuntimeStart(io::Error),
    RuntimeStopped,
    ThreadStart(io::Error),
    TlsConfiguration(NativeTlsConfigError),
    TooManyRedirects,
    Transport(String),
    UnsupportedContentEncoding,
}

impl Display for NativeOutboundHttpError {
    /// Return a bounded transport diagnosis suitable for provider failure redaction.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // avoid including request targets or response bodies in diagnostics
        match self {
            Self::BodyDecode => formatter.write_str("provider response decoding failed"),
            Self::BodyTooLarge { .. } => {
                formatter.write_str("provider response exceeded the byte limit")
            }
            Self::Deadline => formatter.write_str("provider request timed out"),
            Self::InvalidRedirect => formatter.write_str("provider redirect target is invalid"),
            Self::InvalidRequest => formatter.write_str("provider request is invalid"),
            Self::QueueSaturated => formatter.write_str("provider request queue is saturated"),
            Self::RuntimeStart(error) => write!(formatter, "HTTP runtime start failed: {error}"),
            Self::RuntimeStopped => formatter.write_str("HTTP runtime stopped"),
            Self::ThreadStart(error) => write!(formatter, "HTTP thread start failed: {error}"),
            Self::TlsConfiguration(error) => Display::fmt(error, formatter),
            Self::TooManyRedirects => formatter.write_str("provider redirect limit exceeded"),
            Self::Transport(error) => write!(formatter, "provider transport failed: {error}"),
            Self::UnsupportedContentEncoding => {
                formatter.write_str("provider content encoding is unsupported")
            }
        }
    }
}

impl std::error::Error for NativeOutboundHttpError {
    /// Preserve local setup errors without exposing provider content.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // retain only errors with stable local sources
        match self {
            Self::RuntimeStart(error) | Self::ThreadStart(error) => Some(error),
            Self::TlsConfiguration(error) => Some(error),
            _ => None,
        }
    }
}

/// Build a hostname-verifying Rustls client from native platform roots.
///
/// The base config deliberately has no ALPN protocols so the PostgreSQL TLS owner can reuse it.
/// The Hyper connector selects HTTP/1.1 when it is built.
pub(crate) fn native_tls_client_config() -> Result<ClientConfig, NativeTlsConfigError> {
    let native = rustls_native_certs::load_native_certs();
    let mut roots = RootCertStore::empty();
    let (accepted, _) = roots.add_parsable_certificates(native.certs);

    // require at least one usable trust anchor before permitting HTTPS
    if accepted == 0 {
        return Err(NativeTlsConfigError::NativeRootsUnavailable);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| NativeTlsConfigError::ProtocolVersionsUnavailable)
        .map(|builder| builder.with_root_certificates(roots).with_no_client_auth())
}

struct NativeOutboundCommand {
    deadline: Instant,
    maximum_body_bytes: u64,
    request: NativeOutboundHttpRequest,
    result: mpsc::SyncSender<Result<NativeOutboundHttpResponse, NativeOutboundHttpError>>,
}

/// A pooled Hyper client retained on one continuously polled Tokio runtime thread.
pub struct NativeOutboundHttp {
    commands: Option<tokio_mpsc::Sender<NativeOutboundCommand>>,
    runtime_thread: Option<JoinHandle<()>>,
}

impl NativeOutboundHttp {
    /// Start one dedicated current-thread runtime and retain its pooled HTTP client.
    pub fn new() -> Result<Self, NativeOutboundHttpError> {
        let tls = native_tls_client_config().map_err(NativeOutboundHttpError::TlsConfiguration)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(NativeOutboundHttpError::RuntimeStart)?;
        let (commands, receiver) = tokio_mpsc::channel(OUTBOUND_COMMAND_CAPACITY);
        let runtime_thread = thread::Builder::new()
            .name("weather-outbound-http".to_owned())
            .spawn(move || {
                runtime.block_on(run_outbound_runtime(build_client(tls), receiver));
            })
            .map_err(NativeOutboundHttpError::ThreadStart)?;
        Ok(Self {
            commands: Some(commands),
            runtime_thread: Some(runtime_thread),
        })
    }

    /// Execute one full request and response-body exchange on the retained runtime.
    pub fn execute(
        &self,
        request: NativeOutboundHttpRequest,
        timeout: Duration,
        maximum_body_bytes: u64,
    ) -> Result<NativeOutboundHttpResponse, NativeOutboundHttpError> {
        // reject limits which cannot bound a useful response allocation
        if timeout.is_zero() || maximum_body_bytes == 0 || maximum_body_bytes > usize::MAX as u64 {
            return Err(NativeOutboundHttpError::InvalidRequest);
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(NativeOutboundHttpError::InvalidRequest)?;
        let (result, response) = mpsc::sync_channel(1);
        self.commands
            .as_ref()
            .ok_or(NativeOutboundHttpError::RuntimeStopped)?
            .try_send(NativeOutboundCommand {
                deadline,
                maximum_body_bytes,
                request,
                result,
            })
            .map_err(|error| {
                // fail immediately rather than retaining an unbounded request backlog
                if matches!(error, tokio_mpsc::error::TrySendError::Full(_)) {
                    NativeOutboundHttpError::QueueSaturated
                } else {
                    NativeOutboundHttpError::RuntimeStopped
                }
            })?;
        // the waiting caller observes the same monotonic deadline as the runtime
        response
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => NativeOutboundHttpError::Deadline,
                mpsc::RecvTimeoutError::Disconnected => NativeOutboundHttpError::RuntimeStopped,
            })?
    }
}

impl Drop for NativeOutboundHttp {
    /// Stop admission before joining the one dedicated runtime thread.
    fn drop(&mut self) {
        self.commands.take();
        // wait only after closing the command stream
        if let Some(runtime_thread) = self.runtime_thread.take() {
            let _ = runtime_thread.join();
        }
    }
}

/// Source-specific mapping between one logical request and its parsed typed product.
pub trait NativeProviderCodec<Product> {
    /// Construct one request for the controller-provided attempt limits.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError>;

    /// Decode one fully bounded response into the provider's typed product and evidence.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<Product>, ProviderAttemptError>;
}

/// A generic provider-controller exchange over the retained outbound client.
pub struct NativeProviderExchange<'a, Codec> {
    client: &'a NativeOutboundHttp,
    codec: Codec,
}

impl<'a, Codec> NativeProviderExchange<'a, Codec> {
    /// Bind one source codec to the shared pooled transport.
    pub const fn new(client: &'a NativeOutboundHttp, codec: Codec) -> Self {
        Self { client, codec }
    }

    /// Return the source codec after an exchange completes.
    pub const fn codec(&self) -> &Codec {
        &self.codec
    }
}

impl<Product, Codec: NativeProviderCodec<Product>> ProviderIo<Product>
    for NativeProviderExchange<'_, Codec>
{
    /// Sample the shared process clock for retry and run-deadline policy.
    fn now_ms(&mut self) -> i64 {
        system_worker_health_milliseconds().unwrap_or(0)
    }

    /// Generate one unbiased-enough unit jitter sample from approved process entropy.
    fn random_unit(&mut self) -> f64 {
        let mut bytes = [0_u8; 8];
        // retain deterministic zero jitter only when process entropy is unavailable
        if getrandom::fill(&mut bytes).is_err() {
            return 0.0;
        }
        u64::from_le_bytes(bytes) as f64 / u64::MAX as f64
    }

    /// Execute one bounded HTTP attempt and delegate source parsing to its codec.
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<Product>, ProviderAttemptError> {
        let request = self.codec.request(attempt)?;
        let response = self
            .client
            .execute(
                request,
                Duration::from_millis(attempt.timeout_ms),
                attempt.max_body_bytes,
            )
            .map_err(|error| provider_attempt_error(error, attempt.attempt))?;
        self.codec.decode(response)
    }

    /// Sleep synchronously on the worker lane within the controller-admitted duration.
    fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        thread::sleep(Duration::from_millis(sleep.delay_ms));
        Ok(())
    }
}

/// An owned shared exchange for the two independently settled forecast controller lanes.
pub struct OwnedNativeProviderExchange<Codec> {
    client: Arc<NativeOutboundHttp>,
    codec: Codec,
}

impl<Codec> OwnedNativeProviderExchange<Codec> {
    /// Retain one clone of the process client rather than create another runtime or pool.
    pub const fn new(client: Arc<NativeOutboundHttp>, codec: Codec) -> Self {
        Self { client, codec }
    }
}

impl<Product, Codec: NativeProviderCodec<Product>> NativeProviderCodec<Product> for &mut Codec {
    /// Reuse the existing request codec without moving its state between attempts.
    fn request(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<NativeOutboundHttpRequest, ProviderAttemptError> {
        Codec::request(&mut **self, attempt)
    }

    /// Reuse the existing response codec without allocating another decoder.
    fn decode(
        &mut self,
        response: NativeOutboundHttpResponse,
    ) -> Result<ProviderAttemptResponse<Product>, ProviderAttemptError> {
        Codec::decode(&mut **self, response)
    }
}

impl<Product, Codec: NativeProviderCodec<Product>> ProviderIo<Product>
    for OwnedNativeProviderExchange<Codec>
{
    /// Share the same process clock with the borrowed provider exchange.
    fn now_ms(&mut self) -> i64 {
        ProviderIo::<Product>::now_ms(&mut NativeProviderExchange::new(
            &self.client,
            &mut self.codec,
        ))
    }

    /// Share the same entropy owner without copying the codec.
    fn random_unit(&mut self) -> f64 {
        ProviderIo::<Product>::random_unit(&mut NativeProviderExchange::new(
            &self.client,
            &mut self.codec,
        ))
    }

    /// Use the owned process client and independent retained retry state.
    fn execute(
        &mut self,
        attempt: ProviderAttempt,
    ) -> Result<ProviderAttemptResponse<Product>, ProviderAttemptError> {
        NativeProviderExchange::new(&self.client, &mut self.codec).execute(attempt)
    }

    /// Sleep only the calling controller lane, not the shared native HTTP runtime.
    fn sleep(&mut self, sleep: ProviderSleep) -> Result<(), ProviderSleepError> {
        ProviderIo::<Product>::sleep(
            &mut NativeProviderExchange::new(&self.client, &mut self.codec),
            sleep,
        )
    }
}

/// Run the command stream while Hyper's connection tasks remain continuously polled.
async fn run_outbound_runtime(
    client: NativeHyperClient,
    mut commands: tokio_mpsc::Receiver<NativeOutboundCommand>,
) {
    let capacity = Arc::new(tokio::sync::Semaphore::new(OUTBOUND_CONCURRENCY_LIMIT));
    let mut active = tokio::task::JoinSet::new();
    // admit independent bounded exchanges while retaining the same pooled client
    while let Some(command) = commands.recv().await {
        let permit = match Arc::clone(&capacity).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };
        // an expired queued command cannot start a new provider exchange
        if Instant::now() >= command.deadline {
            let _ = command.result.send(Err(NativeOutboundHttpError::Deadline));
            continue;
        }
        let client = client.clone();
        active.spawn(async move {
            let _permit = permit;
            // count queue delay against the same overall request and body deadline
            let outcome = tokio::time::timeout_at(
                command.deadline.into(),
                execute_request(&client, command.request, command.maximum_body_bytes),
            )
            .await
            .map_err(|_| NativeOutboundHttpError::Deadline)
            .and_then(|result| result);
            let _ = command.result.send(outcome);
        });
        // reap completed work without waiting on a different provider request
        while active.try_join_next().is_some() {}
    }
    // finish all already-admitted attempts before dropping the runtime
    while active.join_next().await.is_some() {}
}

/// Build one HTTP-or-HTTPS connector with HTTP/1.1 ALPN selected by Hyper-Rustls.
fn build_client(tls: ClientConfig) -> NativeHyperClient {
    let connector = HttpsConnectorBuilder::new()
        .with_tls_config(tls)
        .https_or_http()
        .enable_http1()
        .build();
    Client::builder(TokioExecutor::new()).build(connector)
}

/// Execute redirects and consume the final response under one attempt deadline.
async fn execute_request(
    client: &NativeHyperClient,
    request: NativeOutboundHttpRequest,
    maximum_body_bytes: u64,
) -> Result<NativeOutboundHttpResponse, NativeOutboundHttpError> {
    let mut body = request.body;
    let mut headers = request.headers;
    let mut method = request.method;
    let redirect_limit = request.redirect_limit;
    let manual_redirect = request.manual_redirect;
    let check_declared_length = request.check_declared_length;
    let mut url = request.url;
    let mut redirects = 0_u8;

    // follow only the Fetch redirect statuses within its finite default limit
    loop {
        let uri = url
            .as_str()
            .parse::<Uri>()
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        // request gzip and deflate because those approved decoders are retained below
        if !headers.contains_key(ACCEPT_ENCODING) {
            headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("gzip, deflate"));
        }
        let mut outbound = Request::builder()
            .method(method.clone())
            .uri(uri)
            .body(Full::new(body.clone()))
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        *outbound.headers_mut() = headers.clone();
        let response = client.request(outbound).await.map_err(transport_error)?;

        // follow an admitted redirect only when the response supplies a location
        if !manual_redirect
            && redirect_status(response.status())
            && let Some(location) = response.headers().get(LOCATION)
        {
            // reject a redirect after the Fetch-compatible finite limit
            if redirects >= redirect_limit {
                return Err(NativeOutboundHttpError::TooManyRedirects);
            }
            let location = location
                .to_str()
                .map_err(|_| NativeOutboundHttpError::InvalidRedirect)?;
            let next = url
                .join(location)
                .map_err(|_| NativeOutboundHttpError::InvalidRedirect)?;
            validate_url(&next).map_err(|_| NativeOutboundHttpError::InvalidRedirect)?;

            // prevent credentials from crossing an origin boundary
            if !same_origin(&url, &next) {
                headers.remove(AUTHORIZATION);
                headers.remove(COOKIE);
                headers.remove("proxy-authorization");
                headers.remove("cookie2");
                headers.remove("host");
            }
            // match Fetch's POST-to-GET redirect rewriting rules
            if response.status() == StatusCode::SEE_OTHER
                && method != Method::GET
                && method != Method::HEAD
                || matches!(
                    response.status(),
                    StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND
                ) && method == Method::POST
            {
                method = Method::GET;
                body = Bytes::new();
                headers.remove(CONTENT_LENGTH);
                headers.remove(CONTENT_TYPE);
                headers.remove("transfer-encoding");
            }
            url = next;
            redirects = redirects.saturating_add(1);
            continue;
        }
        return consume_response(
            response,
            maximum_body_bytes,
            method == Method::HEAD,
            check_declared_length,
        )
        .await;
    }
}

/// Consume one Hyper body and decode only the advertised supported encodings.
async fn consume_response(
    response: hyper::Response<hyper::body::Incoming>,
    maximum_body_bytes: u64,
    head_only: bool,
    check_declared_length: bool,
) -> Result<NativeOutboundHttpResponse, NativeOutboundHttpError> {
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let declared_body_bytes = declared_body_bytes(&headers);

    // reject an honestly declared oversized representation before allocation
    if (head_only || check_declared_length)
        && declared_body_bytes.is_some_and(|length| length > maximum_body_bytes)
    {
        return Err(NativeOutboundHttpError::BodyTooLarge {
            role: ContentBudgetRole::DeclaredLength,
            status,
        });
    }
    // HEAD retains the upstream representation length but does not decode an absent body
    if head_only {
        return Ok(NativeOutboundHttpResponse {
            body: Vec::new(),
            declared_body_bytes,
            headers,
            status,
            streamed_body_bytes: 0,
        });
    }
    let mut incoming = response.into_body();
    let mut encoded = Vec::new();
    // retain only data frames while Hyper owns transfer decoding
    while let Some(frame) = incoming.frame().await {
        let frame = frame.map_err(transport_error)?;
        // ignore protocol metadata frames
        let Ok(data) = frame.into_data() else {
            continue;
        };
        // enforce the raw transport allocation before extending the buffer
        if data.len() as u64 > maximum_body_bytes.saturating_sub(encoded.len() as u64) {
            return Err(NativeOutboundHttpError::BodyTooLarge {
                role: ContentBudgetRole::StreamedBytes,
                status,
            });
        }
        encoded.extend_from_slice(&data);
    }
    let body = decode_content(&headers, encoded, maximum_body_bytes).map_err(|error| {
        // retain the response status when decoded bytes cross the same provider limit
        match error {
            NativeOutboundHttpError::BodyTooLarge { role, .. } => {
                NativeOutboundHttpError::BodyTooLarge { role, status }
            }
            error => error,
        }
    })?;
    Ok(NativeOutboundHttpResponse {
        streamed_body_bytes: body.len() as u64,
        body,
        declared_body_bytes,
        headers,
        status,
    })
}

/// Decode gzip or deflate only when the request advertised those algorithms.
fn decode_content(
    headers: &HeaderMap,
    encoded: Vec<u8>,
    maximum_body_bytes: u64,
) -> Result<Vec<u8>, NativeOutboundHttpError> {
    let Some(encoding) = headers.get(CONTENT_ENCODING) else {
        return Ok(encoded);
    };
    let encoding = encoding
        .to_str()
        .map_err(|_| NativeOutboundHttpError::UnsupportedContentEncoding)?
        .trim()
        .to_ascii_lowercase();

    // decode only the two encodings explicitly advertised by this client
    match encoding.as_str() {
        "gzip" | "x-gzip" => read_decoded(
            GzDecoder::new(encoded.as_slice()),
            maximum_body_bytes,
            ContentBudgetRole::StreamedBytes,
        ),
        "deflate" => decode_deflate(&encoded, maximum_body_bytes),
        "identity" => Ok(encoded),
        _ => Err(NativeOutboundHttpError::UnsupportedContentEncoding),
    }
}

/// Decode zlib-wrapped deflate, falling back to the raw format admitted by Fetch.
fn decode_deflate(
    encoded: &[u8],
    maximum_body_bytes: u64,
) -> Result<Vec<u8>, NativeOutboundHttpError> {
    // try the HTTP-standard zlib wrapper first
    match read_decoded(
        ZlibDecoder::new(encoded),
        maximum_body_bytes,
        ContentBudgetRole::StreamedBytes,
    ) {
        Ok(decoded) => Ok(decoded),
        Err(NativeOutboundHttpError::BodyDecode) => read_decoded(
            DeflateDecoder::new(encoded),
            maximum_body_bytes,
            ContentBudgetRole::StreamedBytes,
        ),
        Err(error) => Err(error),
    }
}

/// Read one synchronous decoder with a byte past the admitted limit as overflow evidence.
fn read_decoded(
    decoder: impl Read,
    maximum_body_bytes: u64,
    role: ContentBudgetRole,
) -> Result<Vec<u8>, NativeOutboundHttpError> {
    let mut decoded = Vec::new();
    decoder
        .take(maximum_body_bytes.saturating_add(1))
        .read_to_end(&mut decoded)
        .map_err(|_| NativeOutboundHttpError::BodyDecode)?;

    // distinguish a successful exact-limit body from decoded overflow
    if decoded.len() as u64 > maximum_body_bytes {
        return Err(NativeOutboundHttpError::BodyTooLarge { role, status: 0 });
    }
    Ok(decoded)
}

/// Map core transport failures into the provider controller's typed attempt boundary.
fn provider_attempt_error(error: NativeOutboundHttpError, attempts: u8) -> ProviderAttemptError {
    // retain response-budget failures as classified invalid payloads
    if let NativeOutboundHttpError::BodyTooLarge { role, status } = error {
        return ProviderAttemptError::Classified(ProviderFailure {
            attempts,
            content_budget_role: Some(role),
            error: ProviderError {
                classification: ProviderFailureClassification::InvalidPayload,
                code: PROVIDER_RESPONSE_TOO_LARGE.to_owned(),
                message: "provider response exceeded the byte limit".to_owned(),
                retry_after_ms: None,
            },
            status: Some(status),
        });
    }
    ProviderAttemptError::Transport {
        message: error.to_string(),
    }
}

/// Validate an absolute outbound URL and exclude user-info credentials.
fn validate_url(url: &Url) -> Result<(), NativeOutboundHttpError> {
    // admit only targets supported by the selected connector
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(())
}

/// Recognize one status followed by Fetch redirect mode.
const fn redirect_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

/// Compare the scheme, host, and effective port of two URLs.
fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

/// Parse one honest content length while ignoring absent or malformed values like Fetch.
fn declared_body_bytes(headers: &HeaderMap) -> Option<u64> {
    headers.get(CONTENT_LENGTH)?.to_str().ok()?.parse().ok()
}

/// Bound one Hyper client failure without retaining request target material.
fn transport_error(error: impl Display) -> NativeOutboundHttpError {
    NativeOutboundHttpError::Transport(error.to_string())
}
