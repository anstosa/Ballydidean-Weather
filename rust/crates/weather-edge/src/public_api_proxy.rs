//! Bounded concrete client for the ordinary public core API proxy.

use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::{
    AdmittedPublicApiUpstreamResponse, ApiUpstreamMethod, PublicApiProjection,
    PublicApiProxyClient, PublicApiProxyClientRequest,
};

/// One already-validated private core API origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedPublicApiOrigin(String);

impl AdmittedPublicApiOrigin {
    /// Retain one origin parsed and approved by the owning URL boundary.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the complete approved origin spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The two transport methods admitted by the read-only proxy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicApiHttpMethod {
    Get,
    Head,
}

/// The deployed fetch redirect behavior for public core API reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicApiRedirectPolicy {
    Manual,
}

/// One exact same-origin public API transport request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicApiHttpRequest {
    pub accept: String,
    pub method: PublicApiHttpMethod,
    pub origin: AdmittedPublicApiOrigin,
    pub pathname: String,
    pub redirect: PublicApiRedirectPolicy,
    pub search: String,
    pub timeout_ms: u64,
}

/// Typed response metadata decoded by the owning HTTP adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicApiHttpResponseHead {
    pub content_length: Option<usize>,
    pub content_type: Option<String>,
    pub status_code: u16,
}

/// Result of one adapter-owned streaming body read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicApiBodyReadOutcome {
    Complete,
    ReceiverStopped,
}

/// One public core API response whose body remains streaming and adapter-owned.
pub trait PublicApiHttpResponse {
    type Error;

    /// Return typed status and headers before any body read.
    fn head(&self) -> PublicApiHttpResponseHead;

    /// Stream response chunks until EOF or bounded receiver refusal.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<PublicApiBodyReadOutcome, Self::Error>;
}

/// HTTP framing, TLS, timeout, and network implementation boundary.
pub trait PublicApiHttpPort {
    type Error;
    type Response: PublicApiHttpResponse<Error = Self::Error>;

    /// Execute one already-admitted same-origin request.
    fn execute(&self, request: &PublicApiHttpRequest) -> Result<Self::Response, Self::Error>;
}

/// Existing settings snapshot and approved JSON projection boundary.
pub trait PublicApiResponseProjector {
    type Error;
    type Snapshot;

    /// Snapshot projection settings before upstream fetch, as in the deployed handler.
    fn prepare(&self, projection: PublicApiProjection) -> Result<Self::Snapshot, Self::Error>;

    /// Project one successful complete bounded JSON response.
    fn project(
        &self,
        projection: PublicApiProjection,
        snapshot: &Self::Snapshot,
        source: &[u8],
    ) -> Result<Vec<u8>, Self::Error>;
}

/// One bounded transport or projection failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicApiProxyError<HttpError, ProjectionError> {
    Http(HttpError),
    Projection(ProjectionError),
    ResponseLimitExceeded,
    ContentLengthExceeded,
}

impl<HttpError: Display, ProjectionError: Display> Display
    for PublicApiProxyError<HttpError, ProjectionError>
{
    /// Render the source-compatible bounded proxy failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve adapter failures and the two exact response-size categories
        match self {
            Self::Http(error) => Display::fmt(error, formatter),
            Self::Projection(error) => Display::fmt(error, formatter),
            Self::ResponseLimitExceeded => {
                formatter.write_str("API response exceeded the edge limit")
            }
            Self::ContentLengthExceeded => {
                formatter.write_str("upstream content length exceeded the edge limit")
            }
        }
    }
}

impl<HttpError, ProjectionError> Error for PublicApiProxyError<HttpError, ProjectionError>
where
    HttpError: Error + 'static,
    ProjectionError: Error + 'static,
{
    /// Return only the injected adapter error as an underlying source.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // expose no invented source for an edge-owned size refusal
        match self {
            Self::Http(error) => Some(error),
            Self::Projection(error) => Some(error),
            Self::ResponseLimitExceeded | Self::ContentLengthExceeded => None,
        }
    }
}

/// Concrete bounded client used by the existing public serving application.
pub struct BoundedPublicApiProxyClient<Http, Projector> {
    http: Http,
    origin: AdmittedPublicApiOrigin,
    projector: Projector,
}

impl<Http, Projector> BoundedPublicApiProxyClient<Http, Projector> {
    /// Bind the approved origin, HTTP adapter, and JSON projection owner.
    pub const fn new(origin: AdmittedPublicApiOrigin, http: Http, projector: Projector) -> Self {
        Self {
            http,
            origin,
            projector,
        }
    }

    /// Recover all bound owners after the serving lifecycle.
    pub fn into_parts(self) -> (AdmittedPublicApiOrigin, Http, Projector) {
        (self.origin, self.http, self.projector)
    }
}

impl<Http, Projector> PublicApiProxyClient for BoundedPublicApiProxyClient<Http, Projector>
where
    Http: PublicApiHttpPort,
    Projector: PublicApiResponseProjector,
{
    type Error = PublicApiProxyError<Http::Error, Projector::Error>;

    /// Execute one bounded same-origin fetch and optional successful JSON projection.
    fn fetch_api(
        &self,
        request: PublicApiProxyClientRequest<'_>,
    ) -> Result<AdmittedPublicApiUpstreamResponse, Self::Error> {
        // snapshot adjustment settings before fetch only for projected representations
        let projection_snapshot = match request.projection {
            PublicApiProjection::None => None,
            PublicApiProjection::Forecast | PublicApiProjection::RainCollection => Some(
                self.projector
                    .prepare(request.projection)
                    .map_err(PublicApiProxyError::Projection)?,
            ),
        };
        let method = match request.upstream_method {
            ApiUpstreamMethod::Get => PublicApiHttpMethod::Get,
            ApiUpstreamMethod::Head => PublicApiHttpMethod::Head,
        };
        let mut response = self
            .http
            .execute(&PublicApiHttpRequest {
                accept: request.accept.to_owned(),
                method,
                origin: self.origin.clone(),
                pathname: request.pathname.to_owned(),
                redirect: PublicApiRedirectPolicy::Manual,
                search: request.search.to_owned(),
                timeout_ms: request.timeout_ms,
            })
            .map_err(PublicApiProxyError::Http)?;
        let head = response.head();
        let successful = (200..300).contains(&head.status_code);

        // preserve HEAD metadata without consuming a body stream
        if request.upstream_method == ApiUpstreamMethod::Head {
            let content_length = head.content_length.unwrap_or(0);

            // reject invalidly large admitted HEAD metadata
            if content_length > request.maximum_response_bytes {
                return Err(PublicApiProxyError::ContentLengthExceeded);
            }
            return Ok(AdmittedPublicApiUpstreamResponse {
                body: Vec::new(),
                content_length: Some(content_length),
                content_type: head.content_type,
                status_code: head.status_code,
                successful,
            });
        }

        let mut source_body = Vec::new();
        let mut receiver_refused = false;
        let outcome = response
            .read_body(&mut |chunk| {
                // reject an accumulated response length that cannot be represented
                let Some(next_length) = source_body.len().checked_add(chunk.len()) else {
                    receiver_refused = true;
                    return false;
                };

                // stop before retaining the first byte beyond the route-specific limit
                if next_length > request.maximum_response_bytes {
                    receiver_refused = true;
                    return false;
                }
                source_body.extend_from_slice(chunk);
                true
            })
            .map_err(PublicApiProxyError::Http)?;

        // require the adapter to honor bounded receiver cancellation
        if receiver_refused || outcome == PublicApiBodyReadOutcome::ReceiverStopped {
            return Err(PublicApiProxyError::ResponseLimitExceeded);
        }
        let body = match (successful, request.projection, projection_snapshot.as_ref()) {
            (true, PublicApiProjection::Forecast, Some(snapshot))
            | (true, PublicApiProjection::RainCollection, Some(snapshot)) => self
                .projector
                .project(request.projection, snapshot, &source_body)
                .map_err(PublicApiProxyError::Projection)?,
            _ => source_body,
        };
        Ok(AdmittedPublicApiUpstreamResponse {
            body,
            content_length: None,
            content_type: head.content_type,
            status_code: head.status_code,
            successful,
        })
    }
}
