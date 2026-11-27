//! Native upstream HTTP and exact origin admission for the public and private edge lanes.

use hyper::{
    Method,
    header::{ACCEPT, CONTENT_TYPE, HeaderValue},
};
use std::{sync::Arc, time::Duration};
use url::Url;
use weather_api::AdmittedMaintenanceArchiveOrigin;
use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedPublicApiOrigin, AdmittedPublicApiUpstreamResponse, ApiUpstreamMethod,
    ArchiveWirePrimitives, EdgeStartupOrigins, PublicAdjustmentJsonCodec, PublicApiProjection,
    PublicApiProxyClient, PublicApiProxyClientRequest, PublicApiProxyError,
    PublicApiResponseProjector,
};
use weather_providers::ContentBudgetRole;
use weather_worker::{TaskFailure, TemperatureCanaryTimeAdapter};

use crate::{
    ApiMaintenanceArchiveOriginOwner, ApiMaintenanceArchivePostTransport,
    ApiMaintenanceArchiveRawPost, ApiMaintenanceArchiveRawResponse, NativeOutboundHttp,
    NativeOutboundHttpError, NativeOutboundHttpRequest, NativeOutboundHttpResponse,
    NativeWirePrimitives, WorkerMaintenanceCapturePost, WorkerMaintenanceCaptureResponse,
    WorkerMaintenanceCaptureTransport, WorkerRainMaintenanceOriginOwner,
    WorkerRainMaintenancePostTransport, WorkerRainMaintenanceRawPost,
    WorkerRainMaintenanceRawResponse,
};

/// One native URL primitive owner shared by process configuration and private relay wiring.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeEdgeOrigins;

/// Admit only the existing origin shape rather than accepting a configurable path prefix.
fn origin(value: &str) -> Result<Url, NativeOutboundHttpError> {
    let origin = Url::parse(value).map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    // retain Node's empty search/hash semantics and require credential-free root origins
    if !matches!(origin.scheme(), "http" | "https")
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some_and(|value| !value.is_empty())
        || origin.path() != "/"
        || origin.query().is_some_and(|value| !value.is_empty())
        || origin.fragment().is_some_and(|value| !value.is_empty())
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(origin)
}

/// Retain the fixed compose service and original disposable loopback origin set.
fn private_origin(value: &str, service: &str) -> Result<Url, NativeOutboundHttpError> {
    let parsed = origin(value)?;
    // require the original private cleartext network and service identity
    if parsed.scheme() != "http"
        || !parsed.host_str().is_some_and(|host| {
            host == service || matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
        })
    {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(parsed)
}

impl EdgeStartupOrigins for NativeEdgeOrigins {
    type Error = NativeOutboundHttpError;
    type XweatherMapOrigin = Url;

    /// Admit the original public core API's selected HTTP or HTTPS origin.
    fn api_origin(&mut self, value: &str) -> Result<AdmittedPublicApiOrigin, Self::Error> {
        origin(value).map(|origin| AdmittedPublicApiOrigin::admitted(origin.as_str()))
    }

    /// Bind the edge's optional private API source to its compose service or loopback.
    fn maintenance_api_origin(
        &mut self,
        value: &str,
    ) -> Result<AdmittedPublicApiOrigin, Self::Error> {
        private_origin(value, "api")
            .map(|origin| AdmittedPublicApiOrigin::admitted(origin.as_str()))
    }

    /// Match the original map provider's HTTPS policy and its IPv4/name loopback exception.
    fn xweather_map_origin(&mut self, value: &str) -> Result<Url, Self::Error> {
        let parsed = origin(value)?;
        // Node's URL.hostname includes IPv6 brackets so the old bare IPv6 exception is not reachable
        if parsed.scheme() == "https"
            || parsed.scheme() == "http"
                && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"))
        {
            Ok(parsed)
        } else {
            Err(NativeOutboundHttpError::InvalidRequest)
        }
    }
}

impl ApiMaintenanceArchiveOriginOwner for NativeEdgeOrigins {
    type Error = NativeOutboundHttpError;

    /// Keep the API archive relay on its original fixed web service origin.
    fn admit_maintenance_archive_origin(
        &mut self,
        value: &str,
    ) -> Result<AdmittedMaintenanceArchiveOrigin, Self::Error> {
        private_origin(value, "web")
            .map(|origin| AdmittedMaintenanceArchiveOrigin::admitted(origin.as_str().to_owned()))
    }
}

impl WorkerRainMaintenanceOriginOwner for NativeEdgeOrigins {
    type Error = NativeOutboundHttpError;

    /// Bind worker-to-API rain and capture calls to their existing private origin.
    fn admit_worker_rain_maintenance_origin(&mut self, raw: &str) -> Result<String, Self::Error> {
        private_origin(raw, "api").map(|origin| origin.as_str().to_owned())
    }
}

/// Build one source-owned relative target and retain its admitted origin after URL resolution.
fn target(base: &Url, pathname: &str, search: &str) -> Result<Url, NativeOutboundHttpError> {
    // preserve the already decoded edge request's URL spelling without switching origins
    if !pathname.starts_with('/') || !search.is_empty() && !search.starts_with('?') {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    let target = base
        .join(&format!("{pathname}{search}"))
        .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
    if target.origin() != base.origin() {
        return Err(NativeOutboundHttpError::InvalidRequest);
    }
    Ok(target)
}

/// Native direct edge proxy over one process-pooled client and the existing response projector.
pub struct NativePublicApiProxy<Projector> {
    origin: Url,
    client: Arc<NativeOutboundHttp>,
    projector: Projector,
}

impl<Projector> NativePublicApiProxy<Projector> {
    /// Bind the already admitted origin and owned response projection without opening another pool.
    pub fn new(
        origin_value: AdmittedPublicApiOrigin,
        client: Arc<NativeOutboundHttp>,
        projector: Projector,
    ) -> Result<Self, NativeOutboundHttpError> {
        Ok(Self {
            origin: origin(origin_value.as_str())?,
            client,
            projector,
        })
    }
}

impl<Projector: PublicApiResponseProjector> PublicApiProxyClient
    for NativePublicApiProxy<Projector>
{
    type Error = PublicApiProxyError<NativeOutboundHttpError, Projector::Error>;

    /// Retain the source ordering of settings snapshot, upstream fetch and optional JSON projection.
    fn fetch_api(
        &self,
        request: PublicApiProxyClientRequest<'_>,
    ) -> Result<AdmittedPublicApiUpstreamResponse, Self::Error> {
        let snapshot = match request.projection {
            PublicApiProjection::None => None,
            projection => Some(
                self.projector
                    .prepare(projection)
                    .map_err(PublicApiProxyError::Projection)?,
            ),
        };
        let head = request.upstream_method == ApiUpstreamMethod::Head;
        let url = target(&self.origin, request.pathname, request.search)
            .map_err(PublicApiProxyError::Http)?;
        let accept = HeaderValue::from_str(request.accept)
            .map_err(|_| PublicApiProxyError::Http(NativeOutboundHttpError::InvalidRequest))?;
        let mut outbound =
            NativeOutboundHttpRequest::new(if head { Method::HEAD } else { Method::GET }, url)
                .map_err(PublicApiProxyError::Http)?
                .header(ACCEPT, accept)
                .manual_redirect();
        // use the source's HEAD metadata check and streamed GET body limit independently
        if !head {
            outbound = outbound.stream_body_limit_only();
        }
        let response = self
            .client
            .execute(
                outbound,
                Duration::from_millis(request.timeout_ms),
                request.maximum_response_bytes as u64,
            )
            .map_err(|error| match error {
                NativeOutboundHttpError::BodyTooLarge {
                    role: ContentBudgetRole::DeclaredLength,
                    ..
                } => PublicApiProxyError::ContentLengthExceeded,
                NativeOutboundHttpError::BodyTooLarge { .. } => {
                    PublicApiProxyError::ResponseLimitExceeded
                }
                error => PublicApiProxyError::Http(error),
            })?;
        let successful = (200..300).contains(&response.status);
        let content_type = response
            .headers
            .get(CONTENT_TYPE)
            .map(|value| node_header_string(value.as_bytes()));
        let content_length = head.then(|| response.declared_body_bytes.unwrap_or(0) as usize);
        // project only the successful response selected by the already snapshotted settings
        let body = match (head, successful, snapshot.as_ref()) {
            (false, true, Some(snapshot)) => self
                .projector
                .project(request.projection, snapshot, &response.body)
                .map_err(PublicApiProxyError::Projection)?,
            _ => response.body,
        };
        Ok(AdmittedPublicApiUpstreamResponse {
            body,
            content_length,
            content_type,
            status_code: response.status,
            successful,
        })
    }
}

/// project raw header octets with the one-byte string semantics used by node fetch
fn node_header_string(value: &[u8]) -> String {
    value.iter().map(|byte| char::from(*byte)).collect()
}

impl PublicAdjustmentJsonCodec for NativeWirePrimitives {
    /// Decode the original Fetch UTF-8 JSON result with the native ordered object owner.
    fn decode(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.decode_json(String::from_utf8_lossy(bytes).as_bytes())
    }

    /// Produce compact source JSON using the same encoder as every other runtime surface.
    fn encode(&self, value: &JsonValue) -> Option<Vec<u8>> {
        let mut bytes = self.encode_ordered_json(value)?;
        (bytes.pop() == Some(b'\n')).then_some(bytes)
    }
}

/// Shared private POST owner with an explicit process-supplied duration and no separate network pool.
#[derive(Clone)]
pub struct NativePrivatePostTransport {
    client: Arc<NativeOutboundHttp>,
    timeout: Duration,
}

impl NativePrivatePostTransport {
    /// Retain the approved client and finite process-selected private request duration.
    pub const fn new(client: Arc<NativeOutboundHttp>, timeout: Duration) -> Self {
        Self { client, timeout }
    }

    /// Send the exact pre-admitted bytes and preserve the response status including 429.
    fn post_raw(
        &self,
        base: Url,
        path: &str,
        content_type: &str,
        body: &[u8],
        maximum_response_body_bytes: usize,
    ) -> Result<NativeOutboundHttpResponse, NativeOutboundHttpError> {
        let url = target(&base, path, "")?;
        let content_type = HeaderValue::from_str(content_type)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        let request = NativeOutboundHttpRequest::new(Method::POST, url)?
            .header(CONTENT_TYPE, content_type)
            .body(body.to_vec());
        self.client
            .execute(request, self.timeout, maximum_response_body_bytes as u64)
    }
}

impl ApiMaintenanceArchivePostTransport for NativePrivatePostTransport {
    type Error = NativeOutboundHttpError;

    /// Forward the existing ordered archive wire bytes to the admitted web origin.
    fn post(
        &mut self,
        request: ApiMaintenanceArchiveRawPost<'_>,
    ) -> Result<ApiMaintenanceArchiveRawResponse, Self::Error> {
        let response = self.post_raw(
            private_origin(request.origin.canonical(), "web")?,
            request.path,
            request.content_type,
            request.body,
            request.maximum_response_body_bytes,
        )?;
        Ok(ApiMaintenanceArchiveRawResponse {
            status: response.status,
            body: response.body,
        })
    }
}

impl WorkerRainMaintenancePostTransport for NativePrivatePostTransport {
    type Error = NativeOutboundHttpError;

    /// Forward the existing rain wire bytes to the admitted API origin.
    fn post(
        &mut self,
        request: WorkerRainMaintenanceRawPost<'_>,
    ) -> Result<WorkerRainMaintenanceRawResponse, Self::Error> {
        let response = self.post_raw(
            private_origin(request.origin.as_str(), "api")?,
            request.path,
            request.content_type,
            request.body,
            request.maximum_response_body_bytes,
        )?;
        Ok(WorkerRainMaintenanceRawResponse {
            status: response.status,
            body: response.body,
        })
    }

    /// Retain the existing bounded capacity delay on the calling worker lane.
    fn wait_for_revision_archive_capacity(&mut self, milliseconds: u64) -> Result<(), Self::Error> {
        std::thread::sleep(Duration::from_millis(milliseconds));
        Ok(())
    }
}

/// One due capture trigger bound to the actual admitted private API origin.
pub struct NativeWorkerMaintenanceCaptureTransport {
    origin: Url,
    transport: NativePrivatePostTransport,
    maximum_response_body_bytes: usize,
}

impl NativeWorkerMaintenanceCaptureTransport {
    /// Retain the source origin and explicit response budget without initializing more sockets.
    pub fn new(
        origin: &str,
        transport: NativePrivatePostTransport,
        maximum_response_body_bytes: usize,
    ) -> Result<Self, NativeOutboundHttpError> {
        Ok(Self {
            origin: private_origin(origin, "api")?,
            transport,
            maximum_response_body_bytes,
        })
    }
}

impl WorkerMaintenanceCaptureTransport for NativeWorkerMaintenanceCaptureTransport {
    type Error = NativeOutboundHttpError;

    /// Encode the two exact generated request fields and retain ordinary response status.
    fn post_json(
        &mut self,
        request: WorkerMaintenanceCapturePost<'_>,
    ) -> Result<WorkerMaintenanceCaptureResponse, Self::Error> {
        let value = JsonValue::Object(vec![
            (
                "dueKey".encode_utf16().collect(),
                JsonValue::String(request.body.dueKey.encode_utf16().collect()),
            ),
            (
                "issuedAt".encode_utf16().collect(),
                JsonValue::String(request.body.issuedAt.encode_utf16().collect()),
            ),
        ]);
        let body = PublicAdjustmentJsonCodec::encode(&NativeWirePrimitives, &value)
            .ok_or(NativeOutboundHttpError::InvalidRequest)?;
        let response = self.transport.post_raw(
            self.origin.clone(),
            request.pathname,
            request.content_type,
            &body,
            self.maximum_response_body_bytes,
        )?;
        // successful source postJson calls parse their response representation before returning
        if (200..300).contains(&response.status)
            && PublicAdjustmentJsonCodec::decode(&NativeWirePrimitives, &response.body).is_none()
        {
            return Err(NativeOutboundHttpError::BodyDecode);
        }
        Ok(WorkerMaintenanceCaptureResponse {
            status_code: response.status,
        })
    }
}

impl TemperatureCanaryTimeAdapter for NativeWirePrimitives {
    /// Use the canonical native UTC owner for worker maintenance and canary receipt timestamps.
    fn utc_string(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        self.format_utc_milliseconds(epoch_ms)
            .ok_or_else(|| TaskFailure {
                code: "invalid_payload".to_owned(),
                attempts: 0,
            })
    }
}
