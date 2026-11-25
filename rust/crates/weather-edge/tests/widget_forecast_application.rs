use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use weather_edge::{
    AdmittedWidgetForecastReceivedAt, AdmittedWidgetForecastSnapshotJson,
    WidgetForecastApplicationBody, WidgetForecastApplicationMethod,
    WidgetForecastApplicationRequest, WidgetForecastApplicationResponse, WidgetForecastClient,
    WidgetForecastClientRequest, WidgetForecastReceivedAtSource, WidgetForecastSnapshotProjector,
    WidgetForecastUpstream, WidgetForecastVersion, handle_widget_forecast_application,
};

const RECEIVED_AT: &str = "2026-09-12T15:15:01.000Z";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureForecast(WidgetForecastVersion);

#[derive(Debug, Default)]
struct FixtureClientState {
    fail: bool,
    requests: Vec<WidgetForecastClientRequest>,
}

#[derive(Debug, Clone, Default)]
struct FixtureClient {
    state: Arc<Mutex<FixtureClientState>>,
    trace: Arc<Mutex<Vec<String>>>,
}

impl FixtureClient {
    /// set whether the next typed fetches fail.
    fn set_fail(&self, fail: bool) {
        self.state.lock().expect("client lock").fail = fail;
    }

    /// remove every previously recorded client request.
    fn clear_requests(&self) {
        self.state.lock().expect("client lock").requests.clear();
    }

    /// return the last typed request observed by the client.
    fn last_request(&self) -> Option<WidgetForecastClientRequest> {
        self.state
            .lock()
            .expect("client lock")
            .requests
            .last()
            .copied()
    }

    /// clear the shared application ordering trace.
    fn clear_trace(&self) {
        self.trace.lock().expect("trace lock").clear();
    }

    /// clone the current application ordering trace.
    fn trace(&self) -> Vec<String> {
        self.trace.lock().expect("trace lock").clone()
    }
}

impl WidgetForecastClient for FixtureClient {
    type Error = ();
    type Forecast = FixtureForecast;

    /// record and return one source-selected typed forecast.
    fn fetch_forecast(
        &self,
        request: WidgetForecastClientRequest,
    ) -> Result<Self::Forecast, Self::Error> {
        let mut state = self.state.lock().expect("client lock");
        state.requests.push(request);
        self.trace.lock().expect("trace lock").push(format!(
            "fetch:{:?}:{:?}:{}:{}",
            request.version, request.upstream, request.timeout_ms, request.maximum_response_bytes
        ));

        // preserve the injected upstream outcome after recording its request
        if state.fail {
            Err(())
        } else {
            Ok(FixtureForecast(request.version))
        }
    }
}

#[derive(Debug, Clone)]
struct FixtureClock {
    fail: Arc<Mutex<bool>>,
    trace: Arc<Mutex<Vec<String>>>,
}

impl FixtureClock {
    /// create one fixed approved receipt source sharing an ordering trace.
    fn new(trace: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            fail: Arc::new(Mutex::new(false)),
            trace,
        }
    }

    /// set whether approved time resolution fails.
    fn set_fail(&self, fail: bool) {
        *self.fail.lock().expect("clock lock") = fail;
    }
}

impl WidgetForecastReceivedAtSource for FixtureClock {
    type Error = ();

    /// return the fixed source receipt after recording its ordering point.
    fn approved_received_at(&self) -> Result<AdmittedWidgetForecastReceivedAt, Self::Error> {
        self.trace
            .lock()
            .expect("trace lock")
            .push("clock".to_owned());

        // preserve the injected canonical-time outcome
        if *self.fail.lock().expect("clock lock") {
            Err(())
        } else {
            Ok(AdmittedWidgetForecastReceivedAt::admitted(RECEIVED_AT))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixtureProjectionMode {
    Ok,
    Error,
    Oversize,
    WrongVersion,
}

#[derive(Debug, Clone)]
struct FixtureProjector {
    bodies: Arc<[Vec<u8>; 3]>,
    mode: Arc<Mutex<FixtureProjectionMode>>,
    trace: Arc<Mutex<Vec<String>>>,
}

impl FixtureProjector {
    /// create one projector backed by actual compiled Node projection bytes.
    fn new(bodies: [Vec<u8>; 3], trace: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            bodies: Arc::new(bodies),
            mode: Arc::new(Mutex::new(FixtureProjectionMode::Ok)),
            trace,
        }
    }

    /// select one injected projection outcome.
    fn set_mode(&self, mode: FixtureProjectionMode) {
        *self.mode.lock().expect("projector lock") = mode;
    }

    /// borrow the actual compiled body for one version.
    fn body(&self, version: WidgetForecastVersion) -> &[u8] {
        // preserve the closed public version ordering
        match version {
            WidgetForecastVersion::V1 => &self.bodies[0],
            WidgetForecastVersion::V2 => &self.bodies[1],
            WidgetForecastVersion::V3 => &self.bodies[2],
        }
    }
}

impl WidgetForecastSnapshotProjector<FixtureForecast> for FixtureProjector {
    type Error = ();

    /// return one actual source projection or injected bounded failure.
    fn project_snapshot(
        &self,
        forecast: &FixtureForecast,
        received_at: &AdmittedWidgetForecastReceivedAt,
        version: WidgetForecastVersion,
    ) -> Result<AdmittedWidgetForecastSnapshotJson, Self::Error> {
        self.trace
            .lock()
            .expect("trace lock")
            .push(format!("project:{version:?}:{}", received_at.as_str()));
        let mode = *self.mode.lock().expect("projector lock");

        // preserve every source-relevant projection result category
        match mode {
            FixtureProjectionMode::Ok => {
                assert_eq!(forecast.0, version);
                Ok(AdmittedWidgetForecastSnapshotJson::admitted(
                    version,
                    self.body(version).to_vec(),
                ))
            }
            FixtureProjectionMode::Error => Err(()),
            FixtureProjectionMode::Oversize => Ok(AdmittedWidgetForecastSnapshotJson::admitted(
                version,
                vec![b'x'; 128 * 1024],
            )),
            FixtureProjectionMode::WrongVersion => Ok(
                AdmittedWidgetForecastSnapshotJson::admitted(other_version(version), b"{}"),
            ),
        }
    }
}

/// return one different public contract version for mismatch coverage.
fn other_version(version: WidgetForecastVersion) -> WidgetForecastVersion {
    // rotate the closed versions without accepting arbitrary values
    match version {
        WidgetForecastVersion::V1 => WidgetForecastVersion::V2,
        WidgetForecastVersion::V2 | WidgetForecastVersion::V3 => WidgetForecastVersion::V1,
    }
}

/// extract one exact line value from the compiled Node oracle.
fn oracle_value<'a>(oracle: &'a str, prefix: &str) -> &'a str {
    oracle
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing oracle line: {prefix}"))
}

/// build one already-parsed application request.
fn request<'a>(
    pathname: &'a str,
    method: WidgetForecastApplicationMethod,
    has_query: bool,
) -> WidgetForecastApplicationRequest<'a> {
    WidgetForecastApplicationRequest {
        has_query,
        method,
        pathname,
    }
}

/// map one typed upstream selection to the exact source path.
fn upstream_path(request: Option<WidgetForecastClientRequest>) -> &'static str {
    // retain no path when route policy rejected before the client
    match request.map(|request| request.upstream) {
        None => "-",
        Some(WidgetForecastUpstream::Daily) => "/api/v1/sites/ballydidean/forecast?days=1",
        Some(WidgetForecastUpstream::Overnight) => {
            "/api/v1/sites/ballydidean/forecast?window=overnight"
        }
    }
}

/// render one response like the actual Node response recorder.
fn response_signature(
    response: &WidgetForecastApplicationResponse,
    upstream: &'static str,
) -> String {
    let (body_state, diagnostic) = match &response.body {
        WidgetForecastApplicationBody::Text(body) => (
            "body",
            format!(":{}", String::from_utf8_lossy(body).trim_end()),
        ),
        WidgetForecastApplicationBody::Json(None) => ("none", String::new()),
        WidgetForecastApplicationBody::Json(Some(_)) => ("body", String::new()),
    };
    format!(
        "{}|{}|{}|{}|{}|{}|{}{}",
        response.status_code,
        response.headers.cache_control,
        response.headers.content_type,
        response.headers.content_length,
        response.headers.allow.unwrap_or("-"),
        body_state,
        upstream,
        diagnostic
    )
}

/// execute one request while isolating its client trace.
fn execute(
    request: WidgetForecastApplicationRequest<'_>,
    client: &FixtureClient,
    clock: &FixtureClock,
    projector: &FixtureProjector,
) -> (
    WidgetForecastApplicationResponse,
    Option<WidgetForecastClientRequest>,
) {
    client.clear_requests();
    client.clear_trace();
    let response = handle_widget_forecast_application(request, client, clock, projector);
    (response, client.last_request())
}

// verifies exact route coordination against the actual compiled Node handler and projectors
#[test]
fn matches_compiled_node_widget_forecast_application() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/widget-forecast-application-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run widget forecast Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let bodies = [
        oracle_value(&node, "body-v1=").as_bytes().to_vec(),
        oracle_value(&node, "body-v2=").as_bytes().to_vec(),
        oracle_value(&node, "body-v3=").as_bytes().to_vec(),
    ];
    assert_eq!(
        oracle_value(&node, "meta="),
        "weather-widget/v1/weather-widget/v2/weather-widget/v3:ballydidean/47.950429954185445/-122.42797012608193:24/31:0.7:0.5/10"
    );

    let trace = Arc::new(Mutex::new(Vec::new()));
    let client = FixtureClient {
        state: Arc::default(),
        trace: Arc::clone(&trace),
    };
    let clock = FixtureClock::new(Arc::clone(&trace));
    let projector = FixtureProjector::new(bodies, trace);
    let v1_path = "/api/v1/sites/ballydidean/widget-forecast";
    let v2_path = "/api/v2/sites/ballydidean/widget-forecast";
    let v3_path = "/api/v3/sites/ballydidean/widget-forecast";

    let cases = [
        (
            "not-found",
            request(
                "/api/v1/sites/other/widget-forecast",
                WidgetForecastApplicationMethod::Get,
                false,
            ),
        ),
        (
            "query",
            request(v1_path, WidgetForecastApplicationMethod::Get, true),
        ),
        (
            "unsupported",
            request(v1_path, WidgetForecastApplicationMethod::Unsupported, false),
        ),
        (
            "v1",
            request(v1_path, WidgetForecastApplicationMethod::Get, false),
        ),
        (
            "v1-head",
            request(v1_path, WidgetForecastApplicationMethod::Head, false),
        ),
        (
            "v2",
            request(v2_path, WidgetForecastApplicationMethod::Get, false),
        ),
        (
            "v3",
            request(v3_path, WidgetForecastApplicationMethod::Get, false),
        ),
    ];

    // compare every ordinary route and method outcome in source order
    for (name, request) in cases {
        let (response, client_request) = execute(request, &client, &clock, &projector);
        assert_eq!(
            response_signature(&response, upstream_path(client_request)),
            oracle_value(&node, &format!("{name}=")),
            "{name}"
        );
    }

    let (v1, _) = execute(
        request(v1_path, WidgetForecastApplicationMethod::Get, false),
        &client,
        &clock,
        &projector,
    );
    let (v1_head, _) = execute(
        request(v1_path, WidgetForecastApplicationMethod::Head, false),
        &client,
        &clock,
        &projector,
    );
    let (v2, _) = execute(
        request(v2_path, WidgetForecastApplicationMethod::Get, false),
        &client,
        &clock,
        &projector,
    );
    let (v3, _) = execute(
        request(v3_path, WidgetForecastApplicationMethod::Get, false),
        &client,
        &clock,
        &projector,
    );
    assert_eq!(
        v1.body,
        WidgetForecastApplicationBody::Json(Some(
            [oracle_value(&node, "body-v1=").as_bytes(), b"\n"].concat()
        ))
    );
    assert_eq!(v1_head.body, WidgetForecastApplicationBody::Json(None));
    assert_eq!(
        v2.body,
        WidgetForecastApplicationBody::Json(Some(
            [oracle_value(&node, "body-v2=").as_bytes(), b"\n"].concat()
        ))
    );
    assert_eq!(
        v3.body,
        WidgetForecastApplicationBody::Json(Some(
            [oracle_value(&node, "body-v3=").as_bytes(), b"\n"].concat()
        ))
    );

    client.set_fail(true);
    let (upstream_failure, client_request) = execute(
        request(v1_path, WidgetForecastApplicationMethod::Get, false),
        &client,
        &clock,
        &projector,
    );
    assert_eq!(
        response_signature(&upstream_failure, upstream_path(client_request)),
        oracle_value(&node, "upstream-failure=")
    );
    assert_eq!(client.trace().len(), 1);
    client.set_fail(false);

    projector.set_mode(FixtureProjectionMode::Error);
    let (projection_failure, client_request) = execute(
        request(v1_path, WidgetForecastApplicationMethod::Get, false),
        &client,
        &clock,
        &projector,
    );
    assert_eq!(
        response_signature(&projection_failure, upstream_path(client_request)),
        oracle_value(&node, "projection-failure=")
    );
    assert_eq!(
        client.trace(),
        vec![
            "fetch:V1:Daily:5000:1048576",
            "clock",
            "project:V1:2026-09-12T15:15:01.000Z"
        ]
    );

    projector.set_mode(FixtureProjectionMode::Oversize);
    let (oversize, client_request) = execute(
        request(v1_path, WidgetForecastApplicationMethod::Get, false),
        &client,
        &clock,
        &projector,
    );
    assert_eq!(
        response_signature(&oversize, upstream_path(client_request)),
        oracle_value(&node, "oversize=")
    );

    projector.set_mode(FixtureProjectionMode::WrongVersion);
    let (wrong_version, _) = execute(
        request(v1_path, WidgetForecastApplicationMethod::Get, false),
        &client,
        &clock,
        &projector,
    );
    assert_eq!(wrong_version.status_code, 502);

    projector.set_mode(FixtureProjectionMode::Ok);
    clock.set_fail(true);
    let (clock_failure, _) = execute(
        request(v1_path, WidgetForecastApplicationMethod::Get, false),
        &client,
        &clock,
        &projector,
    );
    assert_eq!(clock_failure.status_code, 502);
    assert_eq!(client.trace(), vec!["fetch:V1:Daily:5000:1048576", "clock"]);
}
