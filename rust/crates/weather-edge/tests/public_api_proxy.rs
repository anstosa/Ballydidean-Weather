use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_edge::{
    AdmittedPublicApiOrigin, ApiUpstreamMethod, BoundedPublicApiProxyClient,
    PublicApiBodyReadOutcome, PublicApiHttpMethod, PublicApiHttpPort, PublicApiHttpRequest,
    PublicApiHttpResponse, PublicApiHttpResponseHead, PublicApiProjection, PublicApiProxyClient,
    PublicApiProxyClientRequest, PublicApiProxyError, PublicApiRedirectPolicy,
    PublicApiResponseProjector,
};

type Trace = Rc<RefCell<Vec<String>>>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct FixtureError(&'static str);

impl Display for FixtureError {
    /// Render one injected adapter failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for FixtureError {}

#[derive(Clone)]
struct FixtureHttp {
    chunks: Vec<Vec<u8>>,
    head: PublicApiHttpResponseHead,
    request: Rc<RefCell<Option<PublicApiHttpRequest>>>,
    trace: Trace,
}

impl FixtureHttp {
    /// Build one successful JSON response with selected physical chunks.
    fn success(chunks: Vec<Vec<u8>>, trace: Trace) -> Self {
        Self {
            chunks,
            head: PublicApiHttpResponseHead {
                content_length: None,
                content_type: Some("application/x.weather+json".to_owned()),
                status_code: 200,
            },
            request: Rc::new(RefCell::new(None)),
            trace,
        }
    }
}

struct FixtureResponse {
    chunks: VecDeque<Vec<u8>>,
    head: PublicApiHttpResponseHead,
    trace: Trace,
}

impl PublicApiHttpResponse for FixtureResponse {
    type Error = FixtureError;

    /// Return decoded upstream status and header metadata.
    fn head(&self) -> PublicApiHttpResponseHead {
        self.head.clone()
    }

    /// Offer each physical response chunk until EOF or receiver refusal.
    fn read_body(
        &mut self,
        receiver: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<PublicApiBodyReadOutcome, Self::Error> {
        // retain upstream chunk order
        while let Some(chunk) = self.chunks.pop_front() {
            self.trace.borrow_mut().push("read".to_owned());

            // stop immediately after the edge receiver rejects one chunk
            if !receiver(&chunk) {
                self.trace.borrow_mut().push("cancel".to_owned());
                return Ok(PublicApiBodyReadOutcome::ReceiverStopped);
            }
        }
        Ok(PublicApiBodyReadOutcome::Complete)
    }
}

impl PublicApiHttpPort for FixtureHttp {
    type Error = FixtureError;
    type Response = FixtureResponse;

    /// Capture the exact origin, method, representation, redirect, and timeout request.
    fn execute(&self, request: &PublicApiHttpRequest) -> Result<Self::Response, Self::Error> {
        *self.request.borrow_mut() = Some(request.clone());
        self.trace.borrow_mut().push(format!(
            "fetch:{:?}:{}:{}",
            request.method, request.pathname, request.search
        ));
        Ok(FixtureResponse {
            chunks: self.chunks.clone().into(),
            head: self.head.clone(),
            trace: Rc::clone(&self.trace),
        })
    }
}

#[derive(Clone)]
struct FixtureProjector {
    trace: Trace,
}

impl PublicApiResponseProjector for FixtureProjector {
    type Error = FixtureError;
    type Snapshot = &'static str;

    /// Snapshot the selected adjustment settings before fetch.
    fn prepare(&self, projection: PublicApiProjection) -> Result<Self::Snapshot, Self::Error> {
        self.trace
            .borrow_mut()
            .push(format!("prepare:{projection:?}"));
        Ok("settings-snapshot")
    }

    /// Project one successful complete bounded response with the same snapshot.
    fn project(
        &self,
        projection: PublicApiProjection,
        snapshot: &Self::Snapshot,
        source: &[u8],
    ) -> Result<Vec<u8>, Self::Error> {
        assert_eq!(*snapshot, "settings-snapshot");
        assert_eq!(source, br#"{"data":[]}"#);
        self.trace
            .borrow_mut()
            .push(format!("project:{projection:?}"));
        // retain the source fixture's selected projection category
        match projection {
            PublicApiProjection::Forecast => Ok(br#"{"forecast":"filtered"}"#.to_vec()),
            PublicApiProjection::RainCollection => Ok(br#"{"rain":"filtered"}"#.to_vec()),
            PublicApiProjection::None => unreachable!("unprojected responses skip this owner"),
        }
    }
}

/// Build the ordinary public proxy client and its captured request owner.
fn client(
    http: FixtureHttp,
    trace: Trace,
) -> (
    BoundedPublicApiProxyClient<FixtureHttp, FixtureProjector>,
    Rc<RefCell<Option<PublicApiHttpRequest>>>,
) {
    let request = Rc::clone(&http.request);
    (
        BoundedPublicApiProxyClient::new(
            AdmittedPublicApiOrigin::admitted("http://api:4000/"),
            http,
            FixtureProjector { trace },
        ),
        request,
    )
}

/// Build one typed proxy request selected by the existing serving plan.
fn request(
    method: ApiUpstreamMethod,
    projection: PublicApiProjection,
) -> PublicApiProxyClientRequest<'static> {
    PublicApiProxyClientRequest {
        accept: "application/vnd.weather+json",
        maximum_response_bytes: 1024 * 1024,
        pathname: "/api/v1/sites/ballydidean/forecast",
        projection,
        search: "?days=1",
        timeout_ms: 5_000,
        upstream_method: method,
    }
}

/// Prove projected settings snapshot, fetch, bounded read, and projection sequencing.
#[test]
fn projects_a_successful_forecast_after_the_complete_bounded_read() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let http = FixtureHttp::success(
        vec![b"{\"data\":".to_vec(), b"[]}".to_vec()],
        Rc::clone(&trace),
    );
    let (client, captured_request) = client(http, Rc::clone(&trace));
    let response = client
        .fetch_api(request(
            ApiUpstreamMethod::Get,
            PublicApiProjection::Forecast,
        ))
        .expect("project successful forecast");

    assert_eq!(response.body, br#"{"forecast":"filtered"}"#);
    assert_eq!(response.status_code, 200);
    assert!(response.successful);
    assert_eq!(
        trace.borrow().as_slice(),
        [
            "prepare:Forecast",
            "fetch:Get:/api/v1/sites/ballydidean/forecast:?days=1",
            "read",
            "read",
            "project:Forecast",
        ]
    );
    assert_eq!(
        captured_request.borrow().as_ref(),
        Some(&PublicApiHttpRequest {
            accept: "application/vnd.weather+json".to_owned(),
            method: PublicApiHttpMethod::Get,
            origin: AdmittedPublicApiOrigin::admitted("http://api:4000/"),
            pathname: "/api/v1/sites/ballydidean/forecast".to_owned(),
            redirect: PublicApiRedirectPolicy::Manual,
            search: "?days=1".to_owned(),
            timeout_ms: 5_000,
        })
    );
}

/// Prove ordinary HEAD preserves bounded metadata and never reads a response body.
#[test]
fn preserves_bounded_head_metadata_without_reading_the_body() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut http = FixtureHttp::success(vec![b"must-not-read".to_vec()], Rc::clone(&trace));
    http.head.content_length = Some(17);
    let (client, captured_request) = client(http, Rc::clone(&trace));
    let response = client
        .fetch_api(request(ApiUpstreamMethod::Head, PublicApiProjection::None))
        .expect("bounded HEAD metadata");

    assert!(response.body.is_empty());
    assert_eq!(response.content_length, Some(17));
    assert_eq!(
        trace.borrow().as_slice(),
        ["fetch:Head:/api/v1/sites/ballydidean/forecast:?days=1"]
    );
    assert_eq!(
        captured_request
            .borrow()
            .as_ref()
            .expect("captured HEAD")
            .method,
        PublicApiHttpMethod::Head
    );
}

/// Prove non-success bytes remain raw even on a projection-selected route.
#[test]
fn preserves_a_bounded_non_success_body_without_projection() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut http = FixtureHttp::success(vec![b"teapot".to_vec()], Rc::clone(&trace));
    http.head.status_code = 418;
    http.head.content_type = Some("text/plain".to_owned());
    let (client, _) = client(http, Rc::clone(&trace));
    let response = client
        .fetch_api(request(
            ApiUpstreamMethod::Get,
            PublicApiProjection::Forecast,
        ))
        .expect("bounded raw error response");

    assert_eq!(response.body, b"teapot");
    assert!(!response.successful);
    assert_eq!(response.content_type.as_deref(), Some("text/plain"));
    assert_eq!(
        trace.borrow().as_slice(),
        [
            "prepare:Forecast",
            "fetch:Get:/api/v1/sites/ballydidean/forecast:?days=1",
            "read",
        ]
    );
}

/// Prove the first chunk beyond the route-specific cumulative cap is refused.
#[test]
fn refuses_an_oversized_stream_before_retaining_the_extra_chunk() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let http = FixtureHttp::success(
        vec![vec![b'x'; 1024 * 1024], vec![b'y'], vec![b'z']],
        Rc::clone(&trace),
    );
    let (client, _) = client(http, Rc::clone(&trace));
    let error = client
        .fetch_api(request(ApiUpstreamMethod::Get, PublicApiProjection::None))
        .expect_err("reject response above one MiB");

    assert_eq!(
        error,
        PublicApiProxyError::ResponseLimitExceeded::<FixtureError, FixtureError>
    );
    assert_eq!(
        trace.borrow().as_slice(),
        [
            "fetch:Get:/api/v1/sites/ballydidean/forecast:?days=1",
            "read",
            "read",
            "cancel",
        ]
    );
}

/// Compare ordinary method, redirect, HEAD, projection, raw error, and cap behavior with Node.
#[test]
fn matches_deployed_node_proxy_witness() {
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/public-api-proxy-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run public API proxy Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    assert!(node.contains("forecast=200|GET|manual|application/json|projected|none"));
    assert!(node.contains("head=200|HEAD|manual|17|none"));
    assert!(node.contains("raw=418|GET|teapot"));
    assert!(node.contains("oversize=502|upstream unavailable"));
}
