//! Cached home-network display matching over injected DNS and trace transports.

use std::net::Ipv4Addr;
use std::str::FromStr;
use std::sync::{Condvar, Mutex, MutexGuard};

use crate::AdmittedViewerContext;

/// fixed compose service resolved for trusted tunnel peers.
pub const HOME_NETWORK_TUNNEL_SERVICE: &str = "cloudflared";
/// fixed external trace endpoint used for public IPv4 discovery.
pub const HOME_NETWORK_DISCOVERY_URL: &str = "https://www.cloudflare.com/cdn-cgi/trace";
/// maximum successful trace response bytes retained by the matcher.
pub const HOME_NETWORK_MAXIMUM_TRACE_BYTES: usize = 4_096;
/// successful snapshot lifetime and failed-discovery retry interval.
pub const HOME_NETWORK_REFRESH_INTERVAL_MS: u64 = 60_000;
/// overall DNS plus trace discovery deadline.
pub const HOME_NETWORK_DISCOVERY_DEADLINE_MS: u64 = 2_500;
/// per-request HTTP transport deadline inside the overall discovery deadline.
pub const HOME_NETWORK_TRACE_TIMEOUT_MS: u64 = 2_000;

/// one incoming viewer identity projected from the HTTP adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeNetworkMatchRequest<'a> {
    pub connecting_ip: Option<&'a str>,
    pub connecting_ipv6_present: bool,
    pub remote_address: Option<&'a str>,
    pub worker_header_present: bool,
}

/// one fixed IPv4 DNS lookup under the overall refresh deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeNetworkDnsRequest {
    pub family: HomeNetworkAddressFamily,
    pub overall_deadline_ms: u64,
    pub service: &'static str,
}

/// one fixed HTTP trace request under transport and overall deadlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeNetworkTraceRequest {
    pub accept: &'static str,
    pub family: HomeNetworkAddressFamily,
    pub overall_deadline_ms: u64,
    pub timeout_ms: u64,
    pub url: &'static str,
}

/// closed address family required at both discovery boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeNetworkAddressFamily {
    Ipv4,
}

/// one chunked diagnostic response returned by the injected HTTP transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeNetworkTraceResponse {
    pub chunks: Vec<Vec<u8>>,
    pub status_code: u16,
}

/// fixed DNS boundary for the isolated compose tunnel service.
pub trait HomeNetworkDnsPort {
    type Error;

    /// resolve all IPv4 text addresses for the fixed service request.
    fn resolve_ipv4(&self, request: HomeNetworkDnsRequest) -> Result<Vec<String>, Self::Error>;
}

/// fixed HTTP boundary for the external trace document.
pub trait HomeNetworkTracePort {
    type Error;

    /// fetch one bounded trace response under the supplied deadlines.
    fn fetch_trace(
        &self,
        request: HomeNetworkTraceRequest,
    ) -> Result<HomeNetworkTraceResponse, Self::Error>;
}

/// monotonic millisecond clock used for deadlines, expiry, and retry timing.
pub trait HomeNetworkClock {
    /// return the current matcher time in milliseconds.
    fn now_ms(&self) -> u64;
}

/// one sanitized operational warning emitted once per failed refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeNetworkWarning {
    DiscoveryUnavailable,
}

impl HomeNetworkWarning {
    /// return the exact production warning text without visitor or upstream details.
    pub const fn message(self) -> &'static str {
        match self {
            Self::DiscoveryUnavailable => "home network discovery unavailable",
        }
    }
}

/// operational warning sink retained outside matching policy.
pub trait HomeNetworkWarningPort {
    /// record one sanitized matcher warning.
    fn warn(&self, warning: HomeNetworkWarning);
}

/// ordinary trace parsing failures collapsed by the matcher warning boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeNetworkTraceError {
    Address,
    BodyTooLarge,
    Status,
    Utf8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HomeNetworkSnapshot {
    address: Ipv4Addr,
    expires_at_ms: u64,
    peers: Vec<Ipv4Addr>,
}

#[derive(Debug, Default)]
struct HomeNetworkState {
    pending: bool,
    retry_at_ms: u64,
    snapshot: Option<HomeNetworkSnapshot>,
}

/// cached matcher preserving the Node refresh, retry, and coalescing state machine.
pub struct HomeNetworkMatcher<Dns, Trace, Clock, Warnings> {
    clock: Clock,
    dns: Dns,
    ready: Condvar,
    state: Mutex<HomeNetworkState>,
    trace: Trace,
    warnings: Warnings,
}

impl<Dns, Trace, Clock, Warnings> HomeNetworkMatcher<Dns, Trace, Clock, Warnings>
where
    Dns: HomeNetworkDnsPort,
    Trace: HomeNetworkTracePort,
    Clock: HomeNetworkClock,
    Warnings: HomeNetworkWarningPort,
{
    /// bind injected discovery boundaries without network or clock access.
    pub fn new(dns: Dns, trace: Trace, clock: Clock, warnings: Warnings) -> Self {
        Self {
            clock,
            dns,
            ready: Condvar::new(),
            state: Mutex::new(HomeNetworkState::default()),
            trace,
            warnings,
        }
    }

    /// project one viewer context without granting an administrator identity.
    pub fn viewer_context(&self, request: HomeNetworkMatchRequest<'_>) -> AdmittedViewerContext {
        AdmittedViewerContext {
            home_network: self.matches(request),
        }
    }

    /// compare one admitted visitor and socket peer with only fresh discovered evidence.
    pub fn matches(&self, request: HomeNetworkMatchRequest<'_>) -> bool {
        // reject incomplete headers before any discovery work
        let Some(visitor) = request.connecting_ip.and_then(parse_ipv4) else {
            return false;
        };
        let Some(peer) = request.remote_address.and_then(normalize_home_network_peer) else {
            return false;
        };
        // reject worker subrequests and ipv6 forwarding evidence
        if request.worker_header_present || request.connecting_ipv6_present {
            return false;
        }

        let snapshot = self.current(peer);
        let now_ms = self.clock.now_ms();
        snapshot.is_some_and(|snapshot| {
            snapshot.expires_at_ms > now_ms
                && snapshot.peers.contains(&peer)
                && snapshot.address == visitor
        })
    }

    /// return fresh evidence, failed-refresh backoff, or one coalesced refresh result.
    fn current(&self, peer: Ipv4Addr) -> Option<HomeNetworkSnapshot> {
        let mut state = lock_state(&self.state);
        // share the exact in-flight result with every overlapping caller
        if state.pending {
            // tolerate spurious wakeups until the refresh owner publishes its result
            while state.pending {
                state = wait_state(&self.ready, state);
            }
            return state.snapshot.clone();
        }

        // retain successful evidence or the empty failed-refresh backoff
        if self.clock.now_ms() < state.retry_at_ms {
            return state.snapshot.clone();
        }

        state.snapshot = None;
        state.pending = true;
        drop(state);

        let refresh = self.refresh(peer);
        let mut state = lock_state(&self.state);
        // warn once for the one refresh owner without diagnostic details
        if refresh.is_err() {
            self.warnings.warn(HomeNetworkWarning::DiscoveryUnavailable);
        }
        state.snapshot = refresh.unwrap_or(None);
        state.retry_at_ms = self
            .clock
            .now_ms()
            .saturating_add(HOME_NETWORK_REFRESH_INTERVAL_MS);
        state.pending = false;
        let result = state.snapshot.clone();
        self.ready.notify_all();
        result
    }

    /// require both the fixed tunnel peer set and one bounded public IPv4 trace.
    fn refresh(&self, peer: Ipv4Addr) -> Result<Option<HomeNetworkSnapshot>, ()> {
        let overall_deadline_ms = self
            .clock
            .now_ms()
            .saturating_add(HOME_NETWORK_DISCOVERY_DEADLINE_MS);
        let peer_texts = self
            .dns
            .resolve_ipv4(HomeNetworkDnsRequest {
                family: HomeNetworkAddressFamily::Ipv4,
                overall_deadline_ms,
                service: HOME_NETWORK_TUNNEL_SERVICE,
            })
            .map_err(|_| ())?;
        // reject an elapsed deadline before any external trace request
        if self.clock.now_ms() >= overall_deadline_ms {
            return Err(());
        }
        let peers = parse_peer_set(peer_texts).ok_or(())?;
        // reject direct-origin peers without treating them as discovery failures
        if !peers.contains(&peer) {
            return Ok(None);
        }

        let response = self
            .trace
            .fetch_trace(HomeNetworkTraceRequest {
                accept: "text/plain",
                family: HomeNetworkAddressFamily::Ipv4,
                overall_deadline_ms,
                timeout_ms: HOME_NETWORK_TRACE_TIMEOUT_MS,
                url: HOME_NETWORK_DISCOVERY_URL,
            })
            .map_err(|_| ())?;
        let address = read_home_network_trace_ipv4(response).map_err(|_| ())?;
        let now_ms = self.clock.now_ms();
        // prevent a late transport result from creating fresh evidence
        if now_ms >= overall_deadline_ms {
            return Err(());
        }
        Ok(Some(HomeNetworkSnapshot {
            address,
            expires_at_ms: now_ms.saturating_add(HOME_NETWORK_REFRESH_INTERVAL_MS),
            peers,
        }))
    }
}

/// parse one successful trace document while enforcing its streaming byte ceiling.
pub fn read_home_network_trace_ipv4(
    response: HomeNetworkTraceResponse,
) -> Result<Ipv4Addr, HomeNetworkTraceError> {
    // accept only one successful diagnostic response
    if response.status_code != 200 {
        return Err(HomeNetworkTraceError::Status);
    }
    let mut body = Vec::new();
    // enforce the limit before retaining each additional chunk
    for chunk in response.chunks {
        let Some(next_len) = body.len().checked_add(chunk.len()) else {
            return Err(HomeNetworkTraceError::BodyTooLarge);
        };
        if next_len > HOME_NETWORK_MAXIMUM_TRACE_BYTES {
            return Err(HomeNetworkTraceError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&body).map_err(|_| HomeNetworkTraceError::Utf8)?;
    let mut address = None;
    // retain only one documented `ip=` field across lf or crlf framing
    for line in text.lines() {
        let Some(value) = line.strip_prefix("ip=") else {
            continue;
        };
        // reject duplicate or malformed address fields
        if address.is_some() {
            return Err(HomeNetworkTraceError::Address);
        }
        address = parse_ipv4(value);
        if address.is_none() {
            return Err(HomeNetworkTraceError::Address);
        }
    }
    address.ok_or(HomeNetworkTraceError::Address)
}

/// normalize a direct or IPv4-mapped socket address.
pub fn normalize_home_network_peer(address: &str) -> Option<Ipv4Addr> {
    let ipv4 = address.strip_prefix("::ffff:").unwrap_or(address);
    parse_ipv4(ipv4)
}

/// parse one exact ordinary dotted-decimal IPv4 address.
fn parse_ipv4(value: &str) -> Option<Ipv4Addr> {
    Ipv4Addr::from_str(value).ok()
}

/// parse one complete nonempty DNS result without retaining malformed peers.
fn parse_peer_set(values: Vec<String>) -> Option<Vec<Ipv4Addr>> {
    // reject the incomplete discovery result used by no tunnel endpoints
    if values.is_empty() {
        return None;
    }
    let mut peers = Vec::with_capacity(values.len());
    // require every resolver result to be one exact IPv4 address
    for value in values {
        peers.push(parse_ipv4(&value)?);
    }
    Some(peers)
}

/// acquire matcher state even after an unrelated caller panic poisoned the lock.
fn lock_state(state: &Mutex<HomeNetworkState>) -> MutexGuard<'_, HomeNetworkState> {
    state.lock().unwrap_or_else(|error| error.into_inner())
}

/// wait for one coalesced result while recovering a poisoned matcher lock.
fn wait_state<'a>(
    ready: &Condvar,
    state: MutexGuard<'a, HomeNetworkState>,
) -> MutexGuard<'a, HomeNetworkState> {
    ready.wait(state).unwrap_or_else(|error| error.into_inner())
}
