//! Native public edge request ownership before the existing route-specific admissions.

use std::{borrow::Cow, net::SocketAddr, time::Duration};

use hyper::{
    Request, Uri,
    body::Incoming,
    header::{ACCEPT, HeaderMap},
    http::request::Parts,
};
use weather_edge::{
    AdmittedAdminMutation, AdmittedAdminOperator, AdmittedAdminRouteInput,
    AdmittedPropertySensorMutation, AdmittedRemoteAgentsPreviewTarget, AdmittedViewerContext,
    DecodedForecastAdjustmentSettings, EdgeForecastCaptureAdmission, EdgeWireJsonEncoder,
    EdgeWireReply, ForecastResponseEvidenceStore, WeatherEdgeDispatchBoundary,
    WeatherEdgeDispatchPortSet, WeatherEdgeDispatchRequest, XweatherMapProxyRequest,
    handle_edge_wire_request, normalize_forecast_evidence_window,
};

use crate::{NativeHttpBodyReadError, read_native_http_body};

/// Preserve the original request target for the selected WHATWG URL primitive.
pub(crate) fn native_raw_http_target(uri: &Uri) -> Cow<'_, str> {
    // keep absolute and authority forms intact rather than treating them as origin-form paths
    if uri.scheme().is_some() || uri.authority().is_some() {
        Cow::Owned(uri.to_string())
    } else {
        Cow::Borrowed(uri.path_and_query().map_or("/", |target| target.as_str()))
    }
}

/// The URL fields selected by the previous fixed-base edge URL parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedNativeEdgeTarget {
    pub pathname: String,
    pub search: String,
    pub query: Vec<(String, String)>,
}

/// Bind the original fixed-base URL and URLSearchParams primitive without parsing HTTP again.
pub trait NativeEdgeRequestTargetOwner {
    /// Decode against the same fixed edge base and retain submitted query order.
    fn decode_edge_target(&self, raw_target: &str) -> Option<DecodedNativeEdgeTarget>;
}

/// Closed edge URL or body ownership failures for the caller's existing response policy.
#[derive(Debug)]
pub enum NativeEdgeHttpRequestError {
    Target,
    BodyConsumed,
    Body(NativeHttpBodyReadError),
}

/// Retain typed route inputs only after each existing application admission has run.
#[derive(Default)]
pub struct NativeEdgeRequestAdmissions<'a> {
    pub admin_mutation: Option<AdmittedAdminMutation<'a>>,
    pub admin_operator: Option<&'a AdmittedAdminOperator>,
    pub admin_route_input: Option<AdmittedAdminRouteInput<'a>>,
    pub adjustment_settings_body: Option<DecodedForecastAdjustmentSettings>,
    pub map_request: Option<XweatherMapProxyRequest>,
    pub property_sensor_mutation: Option<AdmittedPropertySensorMutation<'a>>,
    pub remote_agents_preview: Option<AdmittedRemoteAgentsPreviewTarget>,
    pub viewer_context: Option<AdmittedViewerContext>,
}

/// Retain Hyper's metadata and one unread body until route-specific admission requests it.
pub struct NativeEdgeHttpRequest {
    parts: Parts,
    peer: SocketAddr,
    target: DecodedNativeEdgeTarget,
    has_query_delimiter: bool,
    body: Option<Incoming>,
}

impl NativeEdgeHttpRequest {
    /// Decode the public edge request using the original fixed-base URL boundary.
    pub fn read<Owner: NativeEdgeRequestTargetOwner>(
        request: Request<Incoming>,
        peer: SocketAddr,
        owner: &Owner,
    ) -> Result<Self, NativeEdgeHttpRequestError> {
        let (parts, body) = request.into_parts();
        let raw_target = native_raw_http_target(&parts.uri);
        let target = owner
            .decode_edge_target(&raw_target)
            .ok_or(NativeEdgeHttpRequestError::Target)?;
        let has_query_delimiter = raw_target.contains('?') || !target.search.is_empty();
        Ok(Self {
            parts,
            peer,
            target,
            has_query_delimiter,
            body: Some(body),
        })
    }

    /// Retain header and method metadata for the original route-specific admission owners.
    pub const fn parts(&self) -> &Parts {
        &self.parts
    }

    /// Preserve the socket's real remote address for the existing viewer-context matcher.
    pub const fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// Borrow exact normalized route and query values from the same URL parser.
    pub const fn target(&self) -> &DecodedNativeEdgeTarget {
        &self.target
    }

    /// Bind only the actual scheduler's process-owned loopback association to this request.
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    pub fn claim_scheduled_capture(
        &mut self,
        registry: &crate::NativeScheduledCaptureRegistry,
    ) -> Option<crate::NativeScheduledCaptureClaim> {
        registry.claim(self.peer.ip(), self.parts.method.as_str(), &mut self.target)
    }

    /// Preserve a bare query delimiter for the caller's exact queryless route admission.
    pub const fn has_query_delimiter(&self) -> bool {
        self.has_query_delimiter
    }

    /// Consume the body only under the existing admitted route's size and time limits.
    pub async fn read_body(
        &mut self,
        maximum: usize,
        deadline: Duration,
    ) -> Result<Vec<u8>, NativeEdgeHttpRequestError> {
        let body = self
            .body
            .take()
            .ok_or(NativeEdgeHttpRequestError::BodyConsumed)?;
        read_native_http_body(body, maximum, deadline)
            .await
            .map_err(NativeEdgeHttpRequestError::Body)
    }

    /// Dispatch real HTTP metadata with the independently admitted private fields.
    pub fn dispatch<'a, Ports, Store, Encoder>(
        &'a self,
        release: &'a str,
        admissions: NativeEdgeRequestAdmissions<'a>,
        ports: Ports,
        capture: EdgeForecastCaptureAdmission,
        store: &mut Store,
        encoder: &mut Encoder,
    ) -> Result<EdgeWireReply<Store::Prepared>, WeatherEdgeDispatchBoundary>
    where
        Ports: WeatherEdgeDispatchPortSet,
        Store: ForecastResponseEvidenceStore,
        Encoder: EdgeWireJsonEncoder,
    {
        let accept = native_accept_header(&self.parts.headers);
        let request = WeatherEdgeDispatchRequest {
            method: self.parts.method.as_str(),
            pathname: &self.target.pathname,
            search: &self.target.search,
            has_query_delimiter: self.has_query_delimiter,
            accept: accept.as_deref(),
            release,
            admin_mutation: admissions.admin_mutation,
            admin_operator: admissions.admin_operator,
            admin_route_input: admissions.admin_route_input,
            adjustment_settings_body: admissions.adjustment_settings_body,
            map_request: admissions.map_request,
            property_sensor_mutation: admissions.property_sensor_mutation,
            remote_agents_preview: admissions.remote_agents_preview,
            viewer_context: admissions.viewer_context,
        };
        handle_edge_wire_request(
            request,
            ports,
            capture,
            // decode the evidence window only when the real forecast response arms capture
            |_| {
                let query = self
                    .target
                    .query
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect::<Vec<_>>();
                normalize_forecast_evidence_window(&query)
            },
            store,
            encoder,
        )
    }
}

/// Preserve Node's ordinary Accept combination without allocating for one valid ASCII value.
fn native_accept_header(headers: &HeaderMap) -> Option<Cow<'_, str>> {
    let values = headers.get_all(ACCEPT);
    let mut accepts = values.iter();
    let first = accepts.next()?;
    let mut first = match first.to_str() {
        Ok(value) => Cow::Borrowed(value),
        // Node exposes parsed HTTP header octets as Latin-1 strings
        Err(_) => Cow::Owned(
            first
                .as_bytes()
                .iter()
                .map(|byte| char::from(*byte))
                .collect(),
        ),
    };
    // combine only additional Accept fields using Node's joined header convention
    for value in accepts {
        let combined = first.to_mut();
        combined.push_str(", ");
        combined.extend(value.as_bytes().iter().map(|byte| char::from(*byte)));
    }
    Some(first)
}
