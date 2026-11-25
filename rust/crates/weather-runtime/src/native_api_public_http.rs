//! Bind incoming HTTP requests to the retained public and observation application owners.

use hyper::{
    Request, Response,
    body::{Bytes, Incoming},
    header::{CONTENT_TYPE, HOST},
};
use weather_api::{
    AdmittedPrivateRouteAuthority, DecodedServiceRequest, QueryParameter, WeatherServiceBoundaries,
    WeatherServiceClock, WeatherServiceRainFallbackDiagnostic, WeatherServiceRepository,
    WeatherServiceWireEncoder,
};

use crate::{
    NativeHttpBody, PooledWeatherServiceRepository, PreparedPublicWeatherApi,
    native_api_http_response,
};

use crate::native_edge_http_request::native_raw_http_target;

const BOUNDARY_ERROR: &[u8] =
    b"{\"error\":{\"code\":\"internal_error\",\"message\":\"Unexpected server error\"}}";

/// Input to the selected Fetch/WHATWG request-target primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeApiRequestTarget<'a> {
    pub method: &'a str,
    pub host: &'a str,
    pub raw_target: &'a str,
}

/// Exact ordered outputs of the previous Request and URLSearchParams boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedNativeApiTarget {
    pub method: String,
    pub pathname: String,
    pub query: Vec<(String, String)>,
}

/// Decode through the selected URL owner instead of substituting HTTP URI path splitting.
pub trait NativeApiRequestTargetOwner {
    /// Preserve normalization, percent decoding, repeated query keys and Fetch method admission.
    fn decode_target(&self, target: NativeApiRequestTarget<'_>) -> Option<DecodedNativeApiTarget>;
}

/// A completely decoded incoming API request, independent of the socket or unread body.
pub struct NativePublicApiHttpRequest {
    target: DecodedNativeApiTarget,
}

impl NativePublicApiHttpRequest {
    /// Adapt raw request metadata exactly as the original Node-to-Fetch server boundary.
    pub fn read<Owner: NativeApiRequestTargetOwner>(
        request: Request<Incoming>,
        owner: &Owner,
    ) -> Result<Self, NativePublicApiIngressError> {
        let (parts, _) = request.into_parts();
        let host = match parts.headers.get(HOST) {
            Some(host) => host.to_str().map_err(|_| NativePublicApiIngressError)?,
            None => "localhost",
        };
        // use the same actual HTTP target representation as the public edge
        let raw_target = native_raw_http_target(&parts.uri);
        let target = owner
            .decode_target(NativeApiRequestTarget {
                method: parts.method.as_str(),
                host,
                raw_target: &raw_target,
            })
            .ok_or(NativePublicApiIngressError)?;
        Ok(Self { target })
    }

    /// Borrow ordered query fields without changing duplicate or empty values.
    fn with_decoded<Result>(
        &self,
        serve: impl FnOnce(DecodedServiceRequest<'_>) -> Result,
    ) -> Result {
        let query = self
            .target
            .query
            .iter()
            .map(|(name, value)| QueryParameter { name, value })
            .collect::<Vec<_>>();
        serve(DecodedServiceRequest {
            method: &self.target.method,
            pathname: &self.target.pathname,
            query: &query,
        })
    }

    /// Route public traffic through its real retained query, clock and encoding owners.
    pub fn serve<Pool, External, Encoder>(
        &self,
        app: &mut PreparedPublicWeatherApi<Pool, External>,
        clock: &mut dyn WeatherServiceClock,
        boundaries: &WeatherServiceBoundaries<'_>,
        encoder: &mut Encoder,
    ) -> Result<Response<NativeHttpBody>, hyper::http::Error>
    where
        PooledWeatherServiceRepository<Pool, External>: WeatherServiceRepository,
        Encoder: WeatherServiceWireEncoder,
    {
        self.with_decoded(|request| {
            native_api_http_response(app.serve(clock, boundaries, request, encoder))
        })
    }

    /// Keep bounded rain degradation reports in the original public application lane.
    pub fn serve_with_rain_diagnostics<Pool, External, Encoder>(
        &self,
        app: &mut PreparedPublicWeatherApi<Pool, External>,
        clock: &mut dyn WeatherServiceClock,
        boundaries: &WeatherServiceBoundaries<'_>,
        encoder: &mut Encoder,
        report: &mut dyn FnMut(WeatherServiceRainFallbackDiagnostic) -> Result<(), ()>,
    ) -> Result<Response<NativeHttpBody>, hyper::http::Error>
    where
        PooledWeatherServiceRepository<Pool, External>: WeatherServiceRepository,
        Encoder: WeatherServiceWireEncoder,
    {
        self.with_decoded(|request| {
            native_api_http_response(
                app.serve_with_rain_diagnostics(clock, boundaries, request, encoder, report),
            )
        })
    }

    /// Dispatch observations only with the separately admitted listener authority.
    pub fn serve_private_observation<Pool, External, Encoder>(
        &self,
        app: &mut PreparedPublicWeatherApi<Pool, External>,
        authority: AdmittedPrivateRouteAuthority,
        clock: &mut dyn WeatherServiceClock,
        boundaries: &WeatherServiceBoundaries<'_>,
        encoder: &mut Encoder,
    ) -> Result<Response<NativeHttpBody>, hyper::http::Error>
    where
        PooledWeatherServiceRepository<Pool, External>: WeatherServiceRepository,
        Encoder: WeatherServiceWireEncoder,
    {
        self.with_decoded(|request| {
            native_api_http_response(
                app.serve_private_observation(authority, clock, boundaries, request, encoder),
            )
        })
    }
}

/// The original server's closed failure before the fetch handler runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativePublicApiIngressError;

impl NativePublicApiIngressError {
    /// Return the original outer server refusal without invoking repository or URL-dependent logic.
    pub fn response(self, head: bool) -> Result<Response<NativeHttpBody>, hyper::http::Error> {
        let body = if head {
            NativeHttpBody::default()
        } else {
            NativeHttpBody::new(Bytes::from_static(BOUNDARY_ERROR))
        };
        Response::builder()
            .status(500)
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(body)
    }
}
