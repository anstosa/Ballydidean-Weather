use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use weather_edge::{
    AdmittedPublicApiUpstreamResponse, AdmittedPublicAssetBytes, ApiUpstreamMethod,
    PublicApiProjection, PublicApiProxyClient, PublicApiProxyClientRequest,
    PublicServingApplicationBody, PublicServingApplicationOutcome, PublicServingApplicationRequest,
    PublicServingApplicationResponse, PublicStaticAssetRead, PublicStaticTemplateRenderRequest,
    PublicStaticTemplateRenderer, StaticAssetRoot, handle_public_static_and_api,
};

#[derive(Debug, Clone)]
struct FixtureAssets {
    fail: Arc<Mutex<bool>>,
}

impl FixtureAssets {
    /// create one successful in-memory asset reader.
    fn available() -> Self {
        Self {
            fail: Arc::new(Mutex::new(false)),
        }
    }

    /// select whether every asset read fails.
    fn set_fail(&self, fail: bool) {
        *self.fail.lock().expect("asset lock") = fail;
    }
}

impl PublicStaticAssetRead for FixtureAssets {
    type Error = ();

    /// return one source-shaped asset from the selected root.
    fn read_asset(
        &self,
        root: StaticAssetRoot,
        relative_path: &str,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error> {
        // preserve the injected file read failure
        if *self.fail.lock().expect("asset lock") {
            return Err(());
        }
        let content = match (root, relative_path) {
            (StaticAssetRoot::Public, "index.html") => b"version=__WEATHER_ASSET_VERSION__;revision=__WEATHER_GIT_REVISION__;digests=__WEATHER_SHELL_ASSET_DIGESTS__;admin=__WEATHER_ADMIN__;analytics=__WEATHER_ANALYTICS__;preload=__WEATHER_ROUTE_PRELOAD__".to_vec(),
            (StaticAssetRoot::Public, "brand/favicon.svg") => b"<svg>fixture</svg>".to_vec(),
            (StaticAssetRoot::Compiled, "client.js") => b"console.log('fixture');\n".to_vec(),
            _ => return Err(()),
        };
        Ok(AdmittedPublicAssetBytes::admitted(content))
    }
}

#[derive(Debug, Clone)]
struct FixtureRenderer {
    forecast: Vec<u8>,
    ordinary: Vec<u8>,
    requests: Arc<Mutex<Vec<(String, bool)>>>,
}

impl FixtureRenderer {
    /// create one renderer from actual compiled Node template outputs.
    fn new(ordinary: Vec<u8>, forecast: Vec<u8>) -> Self {
        Self {
            forecast,
            ordinary,
            requests: Arc::default(),
        }
    }

    /// return every observed template context.
    fn requests(&self) -> Vec<(String, bool)> {
        self.requests.lock().expect("renderer lock").clone()
    }
}

impl PublicStaticTemplateRenderer for FixtureRenderer {
    type Error = ();

    /// return the admitted actual Node rendering for the selected path.
    fn render_template(
        &self,
        source: &AdmittedPublicAssetBytes,
        request: PublicStaticTemplateRenderRequest<'_>,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error> {
        assert!(String::from_utf8_lossy(source.as_bytes()).contains("__WEATHER_ASSET_VERSION__"));
        self.requests
            .lock()
            .expect("renderer lock")
            .push((request.pathname.to_owned(), request.analytics_enabled));
        let content = if request.pathname == "/forecast" {
            &self.forecast
        } else {
            &self.ordinary
        };
        Ok(AdmittedPublicAssetBytes::admitted(content.clone()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixtureApiMode {
    Ok,
    RawError,
    MissingContentType,
    Oversize,
    ProjectedNearLimit,
    HeadOversize,
    TrendsLarge,
    FetchFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedApiRequest {
    accept: String,
    maximum_response_bytes: usize,
    pathname: String,
    projection: PublicApiProjection,
    search: String,
    timeout_ms: u64,
    upstream_method: ApiUpstreamMethod,
}

#[derive(Debug, Clone)]
struct FixtureApi {
    mode: Arc<Mutex<FixtureApiMode>>,
    requests: Arc<Mutex<Vec<ObservedApiRequest>>>,
}

impl FixtureApi {
    /// create one successful typed upstream client.
    fn available() -> Self {
        Self {
            mode: Arc::new(Mutex::new(FixtureApiMode::Ok)),
            requests: Arc::default(),
        }
    }

    /// select one deterministic upstream result category.
    fn set_mode(&self, mode: FixtureApiMode) {
        *self.mode.lock().expect("API mode lock") = mode;
    }

    /// remove every previously observed upstream request.
    fn clear_requests(&self) {
        self.requests.lock().expect("API request lock").clear();
    }

    /// return the last observed upstream request.
    fn last_request(&self) -> Option<ObservedApiRequest> {
        self.requests
            .lock()
            .expect("API request lock")
            .last()
            .cloned()
    }
}

impl PublicApiProxyClient for FixtureApi {
    type Error = ();

    /// return one admitted projected or raw upstream response.
    fn fetch_api(
        &self,
        request: PublicApiProxyClientRequest<'_>,
    ) -> Result<AdmittedPublicApiUpstreamResponse, Self::Error> {
        self.requests
            .lock()
            .expect("API request lock")
            .push(ObservedApiRequest {
                accept: request.accept.to_owned(),
                maximum_response_bytes: request.maximum_response_bytes,
                pathname: request.pathname.to_owned(),
                projection: request.projection,
                search: request.search.to_owned(),
                timeout_ms: request.timeout_ms,
                upstream_method: request.upstream_method,
            });
        let mode = *self.mode.lock().expect("API mode lock");

        // preserve the injected upstream transport failure
        if mode == FixtureApiMode::FetchFailure {
            return Err(());
        }
        let mut response = AdmittedPublicApiUpstreamResponse {
            body: match request.projection {
                PublicApiProjection::Forecast => b"{\"forecast\":\"filtered\"}".to_vec(),
                PublicApiProjection::RainCollection => b"{\"rain\":\"filtered\"}".to_vec(),
                PublicApiProjection::None => b"{\"ok\":true}".to_vec(),
            },
            content_length: None,
            content_type: Some("application/x.weather+json".to_owned()),
            status_code: 200,
            successful: true,
        };

        // apply only the selected source response variant
        match mode {
            FixtureApiMode::Ok => {}
            FixtureApiMode::RawError => {
                response.body = b"teapot".to_vec();
                response.content_type = Some("text/plain".to_owned());
                response.status_code = 418;
                response.successful = false;
            }
            FixtureApiMode::MissingContentType => response.content_type = None,
            FixtureApiMode::Oversize => response.body = vec![b'x'; 1024 * 1024 + 1],
            FixtureApiMode::ProjectedNearLimit => {
                let start = b"{\"data\":[],\"padding\":\"";
                let end = b"\",\"adjustmentSettings\":{\"version\":1}}\n";
                response.body = start.to_vec();
                response.body.resize(1024 * 1024 - 2, b'x');
                response.body.extend_from_slice(end);
            }
            FixtureApiMode::HeadOversize => response.content_length = Some(1024 * 1024 + 1),
            FixtureApiMode::TrendsLarge => response.body = vec![b't'; 1536 * 1024],
            FixtureApiMode::FetchFailure => unreachable!("returned above"),
        }
        Ok(response)
    }
}

/// extract one exact value from the compiled Node oracle.
fn oracle_value<'a>(oracle: &'a str, prefix: &str) -> &'a str {
    oracle
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing oracle line: {prefix}"))
}

/// build one parsed public application request.
fn request<'a>(
    method: &'a str,
    pathname: &'a str,
    search: &'a str,
    accept: Option<&'a str>,
) -> PublicServingApplicationRequest<'a> {
    PublicServingApplicationRequest {
        accept,
        method,
        pathname,
        release: "2026.11.23-1",
        search,
    }
}

/// unwrap one response owned by this application path.
fn handled(outcome: PublicServingApplicationOutcome) -> PublicServingApplicationResponse {
    // reject accidental capture of an existing specialized handler
    match outcome {
        PublicServingApplicationOutcome::Handled(response) => response,
        PublicServingApplicationOutcome::Boundary(boundary) => {
            panic!("unexpected application boundary: {boundary:?}")
        }
    }
}

/// render one response and typed upstream trace like the Node oracle.
fn response_signature(
    response: &PublicServingApplicationResponse,
    request: Option<ObservedApiRequest>,
) -> String {
    let body = match &response.body {
        PublicServingApplicationBody::Bytes(None) => "none",
        PublicServingApplicationBody::Bytes(Some(_)) | PublicServingApplicationBody::Text(_) => {
            "body"
        }
    };
    let (method, target, timeout) = if let Some(request) = request {
        (
            match request.upstream_method {
                ApiUpstreamMethod::Get => "GET",
                ApiUpstreamMethod::Head => "HEAD",
            },
            format!("{}{}", request.pathname, request.search),
            request.timeout_ms.to_string(),
        )
    } else {
        ("-", "-".to_owned(), "-".to_owned())
    };
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        response.status_code,
        response.headers.cache_control,
        response.headers.content_type,
        response.headers.content_length,
        response.headers.vary.unwrap_or("-"),
        response.headers.allow.unwrap_or("-"),
        body,
        method,
        target,
        timeout
    )
}

/// execute one request after clearing its prior client trace.
fn execute(
    request: PublicServingApplicationRequest<'_>,
    assets: &FixtureAssets,
    renderer: &FixtureRenderer,
    api: &FixtureApi,
) -> (PublicServingApplicationResponse, Option<ObservedApiRequest>) {
    api.clear_requests();
    let outcome = handle_public_static_and_api(request, assets, renderer, api);
    (handled(outcome), api.last_request())
}

// verifies static and ordinary API serving against actual production Node functions
#[test]
fn matches_compiled_node_public_static_and_api_application() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/public-static-proxy-application-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run public serving Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let assets = FixtureAssets::available();
    let renderer = FixtureRenderer::new(
        oracle_value(&node, "body-static-html=").as_bytes().to_vec(),
        oracle_value(&node, "body-static-forecast=")
            .as_bytes()
            .to_vec(),
    );
    let api = FixtureApi::available();

    let static_cases = [
        ("static-missing", request("GET", "/missing", "", None)),
        ("static-svg", request("GET", "/brand/favicon.svg", "", None)),
        (
            "static-svg-head",
            request("HEAD", "/brand/favicon.svg", "", None),
        ),
        ("static-html", request("GET", "/", "?ignored=1", None)),
        ("static-html-head", request("HEAD", "/", "", None)),
        ("static-forecast", request("GET", "/forecast", "", None)),
        (
            "static-asset",
            request("GET", "/assets/2026.11.23-1/client.js", "", None),
        ),
    ];

    // compare missing, raw, template, immutable, query, and HEAD response policy
    for (name, request) in static_cases {
        let (response, upstream) = execute(request, &assets, &renderer, &api);
        assert_eq!(
            response_signature(&response, upstream),
            oracle_value(&node, &format!("{name}=")),
            "{name}"
        );
    }
    let (static_svg, _) = execute(
        request("GET", "/brand/favicon.svg", "", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        static_svg.body,
        PublicServingApplicationBody::Bytes(Some(
            oracle_value(&node, "body-static-svg=").as_bytes().to_vec()
        ))
    );
    let (static_asset, _) = execute(
        request("GET", "/assets/2026.11.23-1/client.js", "", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        static_asset.body,
        PublicServingApplicationBody::Bytes(Some(
            [
                oracle_value(&node, "body-static-asset-trimmed=").as_bytes(),
                b"\n",
            ]
            .concat()
        ))
    );
    assert!(
        renderer
            .requests()
            .contains(&("/forecast".to_owned(), true))
    );

    assets.set_fail(true);
    let (read_failure, _) = execute(
        request("GET", "/brand/favicon.svg", "", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        response_signature(&read_failure, None),
        oracle_value(&node, "static-read-failure=")
    );
    assets.set_fail(false);

    let (method, upstream) = execute(
        request("POST", "/api/v1/sites", "", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        response_signature(&method, upstream),
        oracle_value(&node, "proxy-method=")
    );
    let (current, upstream) = execute(
        request(
            "GET",
            "/api/v1/sites/ballydidean/current",
            "?unit=metric",
            Some("application/vnd.weather+json"),
        ),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        response_signature(&current, upstream.clone()),
        oracle_value(&node, "proxy-current=")
    );
    assert_eq!(
        upstream.expect("current request").accept,
        "application/vnd.weather+json"
    );
    assert_eq!(
        current.body,
        PublicServingApplicationBody::Bytes(Some(
            oracle_value(&node, "body-proxy-current=")
                .as_bytes()
                .to_vec()
        ))
    );

    let (current_head, upstream) = execute(
        request("HEAD", "/api/v1/sites/ballydidean/current", "", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        response_signature(&current_head, upstream.clone()),
        oracle_value(&node, "proxy-current-head=")
    );
    let current_head_request = upstream.expect("current HEAD request");
    assert_eq!(current_head_request.accept, "application/json");
    assert_eq!(current_head_request.projection, PublicApiProjection::None);

    let (forecast_head, upstream) = execute(
        request("HEAD", "/api/v1/sites/ballydidean/forecast", "", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        response_signature(&forecast_head, upstream.clone()),
        oracle_value(&node, "proxy-forecast-head=")
    );
    let forecast_head_request = upstream.expect("forecast HEAD request");
    assert_eq!(
        forecast_head_request.upstream_method,
        ApiUpstreamMethod::Get
    );
    assert_eq!(
        forecast_head_request.projection,
        PublicApiProjection::Forecast
    );
    assert!(!forecast_head.track_successful_forecast_response);

    let (forecast_get, upstream) = execute(
        request("GET", "/api/v1/sites/ballydidean/forecast", "?days=1", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        response_signature(&forecast_get, upstream),
        oracle_value(&node, "proxy-forecast-get=")
    );
    assert!(forecast_get.track_successful_forecast_response);
    assert_eq!(
        forecast_get.body,
        PublicServingApplicationBody::Bytes(Some(
            oracle_value(&node, "body-proxy-forecast=")
                .as_bytes()
                .to_vec()
        ))
    );

    // admit an in-budget source whose public settings make the projected body larger
    api.set_mode(FixtureApiMode::ProjectedNearLimit);
    let (expanded, _) = execute(
        request(
            "HEAD",
            "/api/v1/sites/ballydidean/forecast",
            "?days=1",
            None,
        ),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(expanded.status_code, 200);
    assert!(expanded.headers.content_length > 1024 * 1024);
    assert_eq!(expanded.body, PublicServingApplicationBody::Bytes(None));

    let modes = [
        (FixtureApiMode::RawError, "proxy-raw-error"),
        (FixtureApiMode::MissingContentType, "proxy-fallback"),
        (FixtureApiMode::Oversize, "proxy-oversize"),
    ];

    // compare raw non-success, fallback content type, and bounded-body behavior
    for (mode, name) in modes {
        api.set_mode(mode);
        let (response, upstream) = execute(
            request("GET", "/api/v1/sites/ballydidean/current", "", None),
            &assets,
            &renderer,
            &api,
        );
        assert_eq!(
            response_signature(&response, upstream),
            oracle_value(&node, &format!("{name}=")),
            "{name}"
        );
    }

    api.set_mode(FixtureApiMode::HeadOversize);
    let (head_oversize, upstream) = execute(
        request("HEAD", "/api/v1/sites/ballydidean/current", "", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        response_signature(&head_oversize, upstream),
        oracle_value(&node, "proxy-head-oversize=")
    );

    api.set_mode(FixtureApiMode::TrendsLarge);
    let (trends, upstream) = execute(
        request("GET", "/api/v1/sites/ballydidean/trends", "", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        response_signature(&trends, upstream.clone()),
        oracle_value(&node, "proxy-trends=")
    );
    assert_eq!(
        upstream.expect("trends request").maximum_response_bytes,
        2 * 1024 * 1024
    );

    api.set_mode(FixtureApiMode::FetchFailure);
    let (fetch_failure, upstream) = execute(
        request("GET", "/api/v1/sites/ballydidean/current", "", None),
        &assets,
        &renderer,
        &api,
    );
    assert_eq!(
        response_signature(&fetch_failure, upstream),
        oracle_value(&node, "proxy-fetch-failure=")
    );
}
