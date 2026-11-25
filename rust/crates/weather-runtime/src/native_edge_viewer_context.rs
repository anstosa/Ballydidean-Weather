//! Native home-network discovery and request projection for the edge viewer context.

use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use hyper::Method;
use hyper::header::{ACCEPT, ACCEPT_ENCODING, HeaderMap, HeaderValue};
use url::Url;
use weather_edge::{
    AdmittedViewerContext, HOME_NETWORK_DISCOVERY_URL, HOME_NETWORK_MAXIMUM_TRACE_BYTES,
    HOME_NETWORK_TUNNEL_SERVICE, HomeNetworkAddressFamily, HomeNetworkClock, HomeNetworkDnsPort,
    HomeNetworkDnsRequest, HomeNetworkMatchRequest, HomeNetworkMatcher, HomeNetworkTracePort,
    HomeNetworkTraceRequest, HomeNetworkTraceResponse, HomeNetworkWarning, HomeNetworkWarningPort,
};

use crate::{
    NativeEdgeHttpRequest, NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
};

/// Production owner retaining one cached matcher over the shared native HTTP client.
pub struct NativeEdgeViewerContextOwner {
    matcher: HomeNetworkMatcher<
        NativeHomeNetworkDns,
        NativeHomeNetworkTrace,
        NativeHomeNetworkClock,
        NativeHomeNetworkWarnings,
    >,
}

impl NativeEdgeViewerContextOwner {
    /// Bind the system resolver and fixed trace endpoint to one process-shared HTTP client.
    pub fn new(client: Arc<NativeOutboundHttp>) -> Self {
        let clock = NativeHomeNetworkClock::new();
        Self {
            matcher: HomeNetworkMatcher::new(
                NativeHomeNetworkDns::new(clock),
                NativeHomeNetworkTrace::new(client, clock),
                clock,
                NativeHomeNetworkWarnings,
            ),
        }
    }

    /// Project exact Cloudflare headers and the accepted socket peer into display-only context.
    pub fn viewer_context(&self, request: &NativeEdgeHttpRequest) -> AdmittedViewerContext {
        let peer = request.peer().ip().to_string();
        self.matcher.viewer_context(native_match_request(
            &request.parts().headers,
            peer.as_str(),
        ))
    }
}

/// Monotonic process-local clock shared by matcher, DNS, and trace deadline owners.
#[derive(Clone, Copy, Debug)]
pub struct NativeHomeNetworkClock {
    origin: Instant,
}

impl NativeHomeNetworkClock {
    /// Start one monotonic time domain without consulting wall-clock state.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for NativeHomeNetworkClock {
    /// Start the default clock at owner construction.
    fn default() -> Self {
        Self::new()
    }
}

impl HomeNetworkClock for NativeHomeNetworkClock {
    /// Return elapsed monotonic milliseconds saturated to the policy domain.
    fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// Fixed system-resolver boundary for the isolated Compose tunnel service.
#[derive(Clone, Copy, Debug)]
pub struct NativeHomeNetworkDns {
    clock: NativeHomeNetworkClock,
}

impl NativeHomeNetworkDns {
    /// Retain the matcher's exact monotonic deadline domain.
    const fn new(clock: NativeHomeNetworkClock) -> Self {
        Self { clock }
    }
}

impl HomeNetworkDnsPort for NativeHomeNetworkDns {
    type Error = io::Error;

    /// Resolve only fixed-service IPv4 endpoints within the overall discovery deadline.
    fn resolve_ipv4(&self, request: HomeNetworkDnsRequest) -> Result<Vec<String>, Self::Error> {
        // refuse any widened resolver target or address family
        if request.family != HomeNetworkAddressFamily::Ipv4
            || request.service != HOME_NETWORK_TUNNEL_SERVICE
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid home network DNS request",
            ));
        }
        let timeout = remaining(self.clock.now_ms(), request.overall_deadline_ms)
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "DNS deadline elapsed"))?;
        let (result, response) = mpsc::sync_channel(1);
        let _resolver = thread::Builder::new()
            .name("weather-home-network-dns".to_owned())
            .spawn(move || {
                let addresses =
                    (HOME_NETWORK_TUNNEL_SERVICE, 0)
                        .to_socket_addrs()
                        .map(|addresses| {
                            // retain only the requested ipv4 family from system resolver results
                            addresses
                                .filter_map(|address| match address {
                                    SocketAddr::V4(address) => Some(address.ip().to_string()),
                                    SocketAddr::V6(_) => None,
                                })
                                .collect()
                        });
                let _ = result.send(addresses);
            })?;
        // detach only the potentially blocking system lookup like Node's timed-out dns promise
        response
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    io::Error::new(io::ErrorKind::TimedOut, "DNS deadline elapsed")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    io::Error::new(io::ErrorKind::BrokenPipe, "DNS resolver stopped")
                }
            })?
    }
}

/// Fixed external trace transport over the process-shared native HTTP runtime.
pub struct NativeHomeNetworkTrace {
    client: Arc<NativeOutboundHttp>,
    clock: NativeHomeNetworkClock,
}

impl NativeHomeNetworkTrace {
    /// Retain the shared client and the matcher's exact monotonic deadline domain.
    const fn new(client: Arc<NativeOutboundHttp>, clock: NativeHomeNetworkClock) -> Self {
        Self { client, clock }
    }
}

impl HomeNetworkTracePort for NativeHomeNetworkTrace {
    type Error = NativeOutboundHttpError;

    /// Fetch only the fixed identity trace with redirects disabled and a bounded body.
    fn fetch_trace(
        &self,
        request: HomeNetworkTraceRequest,
    ) -> Result<HomeNetworkTraceResponse, Self::Error> {
        // refuse any target, family, or representation outside the retained policy
        if request.family != HomeNetworkAddressFamily::Ipv4
            || request.url != HOME_NETWORK_DISCOVERY_URL
            || request.accept != "text/plain"
        {
            return Err(NativeOutboundHttpError::InvalidRequest);
        }
        let remaining = remaining(self.clock.now_ms(), request.overall_deadline_ms)
            .ok_or(NativeOutboundHttpError::Deadline)?;
        let timeout = remaining.min(Duration::from_millis(request.timeout_ms));
        // reject a zero per-request limit rather than issuing an unbounded exchange
        if timeout.is_zero() {
            return Err(NativeOutboundHttpError::Deadline);
        }
        let url = Url::parse(HOME_NETWORK_DISCOVERY_URL)
            .map_err(|_| NativeOutboundHttpError::InvalidRequest)?;
        let outbound = NativeOutboundHttpRequest::new(Method::GET, url)?
            .header(ACCEPT, HeaderValue::from_static("text/plain"))
            .header(ACCEPT_ENCODING, HeaderValue::from_static("identity"))
            .manual_redirect()
            .stream_body_limit_only();
        let response =
            self.client
                .execute(outbound, timeout, HOME_NETWORK_MAXIMUM_TRACE_BYTES as u64)?;
        Ok(HomeNetworkTraceResponse {
            chunks: vec![response.body],
            status_code: response.status,
        })
    }
}

/// Sanitized production warning owner with no visitor or upstream diagnostic fields.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeHomeNetworkWarnings;

impl HomeNetworkWarningPort for NativeHomeNetworkWarnings {
    /// Emit only the policy-owned stable warning text.
    fn warn(&self, warning: HomeNetworkWarning) {
        eprintln!("{}", warning.message());
    }
}

/// Project only exact native header values and the transport-owned socket address.
fn native_match_request<'a>(
    headers: &'a HeaderMap,
    remote_address: &'a str,
) -> HomeNetworkMatchRequest<'a> {
    HomeNetworkMatchRequest {
        connecting_ip: exact_header(headers, "cf-connecting-ip"),
        connecting_ipv6_present: headers.contains_key("cf-connecting-ipv6"),
        remote_address: Some(remote_address),
        worker_header_present: headers.contains_key("cf-worker"),
    }
}

/// Admit exactly one valid visible header value without choosing among duplicates.
fn exact_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?;
    // reject ambiguous duplicate header fields like Node's invalid joined representation
    if values.next().is_some() {
        return None;
    }
    first.to_str().ok()
}

/// Calculate one positive remaining interval in the shared monotonic domain.
fn remaining(now_ms: u64, deadline_ms: u64) -> Option<Duration> {
    deadline_ms
        .checked_sub(now_ms)
        .filter(|milliseconds| *milliseconds != 0)
        .map(Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    /// Project ordinary native request metadata into the existing viewer matcher contract.
    #[test]
    fn projects_native_viewer_request_metadata() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", HeaderValue::from_static("203.0.113.20"));
        let peer = IpAddr::V4(Ipv4Addr::new(172, 20, 0, 2)).to_string();

        assert_eq!(
            native_match_request(&headers, &peer),
            HomeNetworkMatchRequest {
                connecting_ip: Some("203.0.113.20"),
                connecting_ipv6_present: false,
                remote_address: Some("172.20.0.2"),
                worker_header_present: false,
            }
        );
    }

    /// Preserve the exact positive interval and elapsed-deadline outcomes.
    #[test]
    fn calculates_shared_deadline_interval() {
        assert_eq!(remaining(100, 2_500), Some(Duration::from_millis(2_400)));
        assert_eq!(remaining(2_500, 2_500), None);
        assert_eq!(remaining(2_501, 2_500), None);
    }
}
