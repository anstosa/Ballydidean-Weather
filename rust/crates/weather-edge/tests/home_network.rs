use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use weather_edge::{
    HOME_NETWORK_DISCOVERY_DEADLINE_MS, HOME_NETWORK_DISCOVERY_URL,
    HOME_NETWORK_MAXIMUM_TRACE_BYTES, HOME_NETWORK_TRACE_TIMEOUT_MS, HOME_NETWORK_TUNNEL_SERVICE,
    HomeNetworkAddressFamily, HomeNetworkClock, HomeNetworkDnsPort, HomeNetworkDnsRequest,
    HomeNetworkMatchRequest, HomeNetworkMatcher, HomeNetworkTracePort, HomeNetworkTraceRequest,
    HomeNetworkTraceResponse, HomeNetworkWarning, HomeNetworkWarningPort,
    read_home_network_trace_ipv4,
};

const HOME_ADDRESS: &str = "203.0.113.20";
const TUNNEL_ADDRESS: &str = "172.20.0.2";

#[derive(Clone, Default)]
struct FixtureClock(Arc<AtomicU64>);

impl FixtureClock {
    /// set one deterministic matcher time.
    fn set(&self, now_ms: u64) {
        self.0.store(now_ms, Ordering::SeqCst);
    }
}

impl HomeNetworkClock for FixtureClock {
    /// return the current deterministic matcher time.
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
struct FixtureDns {
    calls: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<HomeNetworkDnsRequest>>>,
}

impl FixtureDns {
    /// create one stable compose DNS fixture.
    fn new() -> Self {
        Self {
            calls: Arc::default(),
            requests: Arc::default(),
        }
    }
}

impl HomeNetworkDnsPort for FixtureDns {
    type Error = ();

    /// resolve the ordinary tunnel fixture and retain its typed request.
    fn resolve_ipv4(&self, request: HomeNetworkDnsRequest) -> Result<Vec<String>, Self::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().expect("dns requests").push(request);
        Ok(vec![TUNNEL_ADDRESS.to_owned()])
    }
}

#[derive(Clone)]
struct FixtureTrace {
    address: Arc<Mutex<String>>,
    calls: Arc<AtomicUsize>,
    fail: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<HomeNetworkTraceRequest>>>,
}

impl FixtureTrace {
    /// create one successful trace fixture.
    fn new() -> Self {
        Self {
            address: Arc::new(Mutex::new(HOME_ADDRESS.to_owned())),
            calls: Arc::default(),
            fail: Arc::default(),
            requests: Arc::default(),
        }
    }

    /// replace the observed public address.
    fn set_address(&self, address: &str) {
        *self.address.lock().expect("trace address") = address.to_owned();
    }

    /// select one ordinary transport outage.
    fn set_fail(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }
}

impl HomeNetworkTracePort for FixtureTrace {
    type Error = ();

    /// return one chunked trace document or the selected transport outage.
    fn fetch_trace(
        &self,
        request: HomeNetworkTraceRequest,
    ) -> Result<HomeNetworkTraceResponse, Self::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().expect("trace requests").push(request);
        // expose the ordinary failed discovery without transport details
        if self.fail.load(Ordering::SeqCst) {
            return Err(());
        }
        let address = self.address.lock().expect("trace address").clone();
        Ok(HomeNetworkTraceResponse {
            chunks: vec![
                b"fl=123\n".to_vec(),
                format!("ip={address}\ncolo=SEA\n").into_bytes(),
            ],
            status_code: 200,
        })
    }
}

#[derive(Clone, Default)]
struct FixtureWarnings(Arc<Mutex<Vec<HomeNetworkWarning>>>);

impl HomeNetworkWarningPort for FixtureWarnings {
    /// retain one sanitized operational warning.
    fn warn(&self, warning: HomeNetworkWarning) {
        self.0.lock().expect("warnings").push(warning);
    }
}

#[derive(Clone)]
struct BlockingTrace {
    calls: Arc<AtomicUsize>,
    gate: Arc<Barrier>,
}

impl HomeNetworkTracePort for BlockingTrace {
    type Error = ();

    /// hold one refresh long enough for overlapping callers to coalesce.
    fn fetch_trace(
        &self,
        _request: HomeNetworkTraceRequest,
    ) -> Result<HomeNetworkTraceResponse, Self::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.gate.wait();
        Ok(HomeNetworkTraceResponse {
            chunks: vec![format!("ip={HOME_ADDRESS}\n").into_bytes()],
            status_code: 200,
        })
    }
}

/// construct one ordinary viewer request.
fn request<'a>(visitor: &'a str, peer: &'a str) -> HomeNetworkMatchRequest<'a> {
    HomeNetworkMatchRequest {
        connecting_ip: Some(visitor),
        connecting_ipv6_present: false,
        remote_address: Some(peer),
        worker_header_present: false,
    }
}

// verify ipv4 matching and fixed discovery requests
#[test]
fn admits_direct_and_mapped_tunnel_peers_into_viewer_context() {
    let clock = FixtureClock::default();
    let dns = FixtureDns::new();
    let trace = FixtureTrace::new();
    let warnings = FixtureWarnings::default();
    let matcher = HomeNetworkMatcher::new(dns.clone(), trace.clone(), clock, warnings);

    assert!(matcher.matches(request(HOME_ADDRESS, TUNNEL_ADDRESS)));
    assert!(
        matcher
            .viewer_context(request(HOME_ADDRESS, "::ffff:172.20.0.2"))
            .home_network
    );
    assert!(!matcher.matches(request("198.51.100.80", TUNNEL_ADDRESS)));
    assert!(!matcher.matches(request(HOME_ADDRESS, "172.20.0.3")));
    assert_eq!(dns.calls.load(Ordering::SeqCst), 1);
    assert_eq!(trace.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *dns.requests.lock().expect("dns requests"),
        vec![HomeNetworkDnsRequest {
            family: HomeNetworkAddressFamily::Ipv4,
            overall_deadline_ms: HOME_NETWORK_DISCOVERY_DEADLINE_MS,
            service: HOME_NETWORK_TUNNEL_SERVICE,
        }]
    );
    assert_eq!(
        *trace.requests.lock().expect("trace requests"),
        vec![HomeNetworkTraceRequest {
            accept: "text/plain",
            family: HomeNetworkAddressFamily::Ipv4,
            overall_deadline_ms: HOME_NETWORK_DISCOVERY_DEADLINE_MS,
            timeout_ms: HOME_NETWORK_TRACE_TIMEOUT_MS,
            url: HOME_NETWORK_DISCOVERY_URL,
        }]
    );
}

// verify ttl refresh, backoff, and stale-result removal
#[test]
fn refreshes_expired_addresses_and_never_reuses_failed_evidence() {
    let clock = FixtureClock::default();
    let dns = FixtureDns::new();
    let trace = FixtureTrace::new();
    let warnings = FixtureWarnings::default();
    let matcher = HomeNetworkMatcher::new(dns, trace.clone(), clock.clone(), warnings.clone());

    assert!(matcher.matches(request(HOME_ADDRESS, TUNNEL_ADDRESS)));
    assert!(matcher.matches(request(HOME_ADDRESS, TUNNEL_ADDRESS)));
    assert_eq!(trace.calls.load(Ordering::SeqCst), 1);

    clock.set(60_000);
    trace.set_address("203.0.113.21");
    assert!(!matcher.matches(request(HOME_ADDRESS, TUNNEL_ADDRESS)));
    assert!(matcher.matches(request("203.0.113.21", TUNNEL_ADDRESS)));
    assert_eq!(trace.calls.load(Ordering::SeqCst), 2);

    clock.set(120_000);
    trace.set_fail(true);
    assert!(!matcher.matches(request("203.0.113.21", TUNNEL_ADDRESS)));
    assert!(!matcher.matches(request("203.0.113.21", TUNNEL_ADDRESS)));
    assert_eq!(trace.calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        *warnings.0.lock().expect("warnings"),
        vec![HomeNetworkWarning::DiscoveryUnavailable]
    );

    clock.set(180_000);
    trace.set_fail(false);
    assert!(matcher.matches(request("203.0.113.21", TUNNEL_ADDRESS)));
    assert_eq!(trace.calls.load(Ordering::SeqCst), 4);
}

// verify simultaneous callers share one refresh
#[test]
fn coalesces_overlapping_viewers_behind_one_discovery() {
    let dns = FixtureDns::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Barrier::new(2));
    let matcher = Arc::new(HomeNetworkMatcher::new(
        dns,
        BlockingTrace {
            calls: Arc::clone(&calls),
            gate: Arc::clone(&gate),
        },
        FixtureClock::default(),
        FixtureWarnings::default(),
    ));
    let visitors = [HOME_ADDRESS, "198.51.100.80", HOME_ADDRESS];
    let handles = visitors.map(|visitor| {
        let matcher = Arc::clone(&matcher);
        std::thread::spawn(move || matcher.matches(request(visitor, TUNNEL_ADDRESS)))
    });

    gate.wait();
    let results = handles.map(|handle| handle.join().expect("viewer thread"));
    assert_eq!(results, [true, false, true]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// verify the exact normal trace byte ceiling
#[test]
fn reads_one_chunked_ipv4_trace_at_the_4096_byte_boundary() {
    let prefix = format!("fl=123\r\nip={HOME_ADDRESS}\r\n");
    let mut body = prefix.into_bytes();
    body.resize(HOME_NETWORK_MAXIMUM_TRACE_BYTES, b'x');
    let response = HomeNetworkTraceResponse {
        chunks: vec![body[..2_000].to_vec(), body[2_000..].to_vec()],
        status_code: 200,
    };

    assert_eq!(
        read_home_network_trace_ipv4(response)
            .expect("bounded trace")
            .to_string(),
        HOME_ADDRESS
    );
}

// verify normal behavior against the actual node matcher
#[test]
fn matches_node_cache_refresh_warning_coalescing_and_trace_behavior() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/home-network-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run home network oracle");

    assert!(
        output.status.success(),
        "oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("oracle stdout"),
        concat!(
            "direct=true\n",
            "mapped=true\n",
            "away=false\n",
            "refresh_calls=3\n",
            "stale_after_failure=false\n",
            "warnings=home network discovery unavailable\n",
            "coalesced_calls=1\n",
            "trace_bytes=4096\n",
        )
    );
}
