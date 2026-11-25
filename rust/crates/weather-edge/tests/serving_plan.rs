use std::path::PathBuf;
use std::process::Command;

use weather_edge::{
    ApiContentTypePlan, ApiUpstreamMethod, DelegatedServingBoundary, PublicApiProjection,
    PublicServingPlan, PublicServingRejection, StaticAssetRoot, WidgetForecastUpstream,
    WidgetForecastVersion, plan_public_request,
};

/// Invoke one extracted Node serving helper.
fn node_oracle(arguments: &[&str]) -> String {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/web_server_oracle.mjs");
    let output = Command::new("node")
        .arg(script)
        .args(arguments)
        .output()
        .expect("Node oracle must start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Node oracle must emit UTF-8")
}

/// Prove representative fixed and release assets against the production maps.
#[test]
fn matches_static_asset_allowlist_and_metadata() {
    let cases = [
        "/",
        "/logs/",
        "/privacy",
        "/manifest.webmanifest",
        "/service-worker.js",
        "/brand/favicon.svg",
        "/brand/ballydidean-weather-icon-192.png",
        "/weather-icons/09-heavy-rain.svg",
        "/fonts/google-sans-flex-latin.woff2",
        "/fonts/LICENSE-material-symbols.txt",
        "/assets/2026.11.23-1/styles.css",
        "/assets/2026.11.23-1/client.js",
        "/assets/2026.11.23-1/forecast-correction-policy.js",
    ];
    // compare exact allowlist metadata from both source roots
    for pathname in cases {
        let PublicServingPlan::StaticAsset(plan) =
            plan_public_request("2026.11.23-1", "GET", pathname, false)
        else {
            panic!("expected static asset for {pathname}");
        };
        let root = match plan.root {
            StaticAssetRoot::Public => "public",
            StaticAssetRoot::Compiled => "compiled",
        };
        let actual = format!(
            "{root}/{}\t{}\t{}\t{}\t{}",
            plan.relative_path,
            plan.cache_control,
            plan.content_type,
            plan.template,
            plan.analytics_allowed,
        );
        assert_eq!(
            actual,
            node_oracle(&["asset", "2026.11.23-1", pathname]),
            "{pathname}",
        );
    }

    assert_eq!(
        plan_public_request(
            "2026.11.23-1",
            "GET",
            "/assets/2026.11.22-1/client.js",
            false,
        ),
        PublicServingPlan::Reject(PublicServingRejection::NotFound),
    );
    assert_eq!(
        plan_public_request("2026.11.23-1", "GET", "/unknown", false),
        PublicServingPlan::Reject(PublicServingRejection::NotFound),
    );
    let PublicServingPlan::StaticAsset(head) =
        plan_public_request("2026.11.23-1", "HEAD", "/privacy/", false)
    else {
        panic!("expected HEAD asset");
    };
    assert!(head.omit_body);
}

/// Prove closed widget matching and route-order rejection semantics.
#[test]
fn matches_widget_route_classification() {
    let paths = [
        ("/api/v1/sites/ballydidean/widget-forecast", true),
        ("/api/v2/sites/ballydidean/widget-forecast", true),
        ("/api/v3/sites/ballydidean/widget-forecast", true),
        ("/api/v1/sites/other/widget-forecast", true),
        ("/api/v2/sites/ballydidean/widget-forecast/extra", true),
        ("/api/v3/sites/ballydidean/widget-forecastx", false),
    ];
    // compare the exact production widget matcher including rejected near matches
    for (pathname, expected) in paths {
        assert_eq!(
            node_oracle(&["widget-path", pathname]) == "true",
            expected,
            "{pathname}",
        );
    }

    let PublicServingPlan::Widget(v1) = plan_public_request(
        "development",
        "GET",
        "/api/v1/sites/ballydidean/widget-forecast",
        false,
    ) else {
        panic!("expected v1 widget");
    };
    assert_eq!(v1.version, WidgetForecastVersion::V1);
    assert_eq!(v1.upstream, WidgetForecastUpstream::Daily);
    assert_eq!(v1.cache_control, "no-store");

    let PublicServingPlan::Widget(v3) = plan_public_request(
        "development",
        "HEAD",
        "/api/v3/sites/ballydidean/widget-forecast",
        false,
    ) else {
        panic!("expected v3 widget");
    };
    assert_eq!(v3.version, WidgetForecastVersion::V3);
    assert_eq!(v3.upstream, WidgetForecastUpstream::Overnight);
    assert!(v3.omit_body);

    assert_eq!(
        plan_public_request(
            "development",
            "POST",
            "/api/v1/sites/ballydidean/widget-forecast",
            true,
        ),
        PublicServingPlan::Reject(PublicServingRejection::BadRequest),
    );
    assert_eq!(
        plan_public_request(
            "development",
            "GET",
            "/api/v1/sites/other/widget-forecast",
            false,
        ),
        PublicServingPlan::Reject(PublicServingRejection::NotFound),
    );
    assert_eq!(
        plan_public_request(
            "development",
            "POST",
            "/api/v2/sites/ballydidean/widget-forecast/extra",
            false,
        ),
        PublicServingPlan::Reject(PublicServingRejection::NotFound),
    );
    assert_eq!(
        plan_public_request(
            "development",
            "POST",
            "/api/v3/sites/ballydidean/widget-forecastx",
            false,
        ),
        PublicServingPlan::Reject(PublicServingRejection::MethodNotAllowed),
    );
}

/// Prove API forwarding metadata and recorded-source route precedence.
#[test]
fn plans_read_only_api_forwarding() {
    let PublicServingPlan::ApiProxy(current) = plan_public_request(
        "development",
        "HEAD",
        "/api/v1/sites/ballydidean/current",
        false,
    ) else {
        panic!("expected current proxy");
    };
    assert_eq!(current.upstream_method, ApiUpstreamMethod::Head);
    assert_eq!(current.projection, PublicApiProjection::None);
    assert_eq!(
        current.upstream_success_cache_control,
        node_api_cache("/api/v1/sites/ballydidean/current")
    );
    assert_eq!(current.upstream_non_success_cache_control, "no-store");
    assert_eq!(current.maximum_response_bytes, 1024 * 1024);
    assert_eq!(
        current.timeout_ms,
        node_api_timeout("/api/v1/sites/ballydidean/current")
    );
    assert_eq!(
        current.upstream_success_content_type,
        ApiContentTypePlan::UpstreamOrJson
    );
    assert_eq!(
        current.upstream_non_success_content_type,
        ApiContentTypePlan::UpstreamOrJson
    );
    assert!(current.vary_accept);
    assert!(!current.track_successful_forecast_response);
    assert!(current.omit_body);

    let PublicServingPlan::ApiProxy(forecast) = plan_public_request(
        "development",
        "HEAD",
        "/api/v1/sites/ballydidean/forecast",
        false,
    ) else {
        panic!("expected forecast proxy");
    };
    assert_eq!(forecast.upstream_method, ApiUpstreamMethod::Get);
    assert_eq!(forecast.projection, PublicApiProjection::Forecast);
    assert_eq!(forecast.upstream_success_cache_control, "no-store");
    assert_eq!(
        forecast.upstream_success_content_type,
        ApiContentTypePlan::Json
    );
    assert_eq!(
        forecast.upstream_non_success_content_type,
        ApiContentTypePlan::UpstreamOrJson
    );
    assert!(!forecast.track_successful_forecast_response);

    let PublicServingPlan::ApiProxy(forecast_get) = plan_public_request(
        "development",
        "GET",
        "/api/v1/sites/ballydidean/forecast",
        false,
    ) else {
        panic!("expected GET forecast proxy");
    };
    assert!(forecast_get.track_successful_forecast_response);

    let PublicServingPlan::ApiProxy(trends) =
        plan_public_request("development", "GET", "/api/v1/sites/other/trends", true)
    else {
        panic!("expected trends proxy");
    };
    assert_eq!(trends.maximum_response_bytes, 2 * 1024 * 1024);
    assert_eq!(
        trends.timeout_ms,
        node_api_timeout("/api/v1/sites/other/trends")
    );
    assert_eq!(
        trends.upstream_success_cache_control,
        node_api_cache("/api/v1/sites/other/trends")
    );

    let PublicServingPlan::RecordedForecast(observations) = plan_public_request(
        "development",
        "GET",
        "/api/v1/sites/ballydidean/forecast-observations",
        false,
    ) else {
        panic!("expected recorded forecast action");
    };
    assert!(!observations.solar_cloud_evidence);
    assert_eq!(
        observations.cache_control,
        "public, max-age=60, s-maxage=60"
    );
    assert_eq!(
        plan_public_request(
            "development",
            "GET",
            "/api/v1/sites/ballydidean/solar-cloud-evidence",
            true,
        ),
        PublicServingPlan::Reject(PublicServingRejection::UnsupportedQuery),
    );
    assert_eq!(
        plan_public_request("development", "POST", "/api/v1/sites", false),
        PublicServingPlan::Reject(PublicServingRejection::MethodNotAllowed),
    );
}

/// Keep authorization and transport-owned routes opaque to this planner.
#[test]
fn preserves_delegated_control_boundaries() {
    let cases = [
        (
            "/api/v1/viewer-context",
            DelegatedServingBoundary::ViewerContext,
        ),
        ("/admin/login", DelegatedServingBoundary::AdminLogin),
        ("/admin", DelegatedServingBoundary::AdminPage),
        (
            "/api/v1/admin/sites/ballydidean/forecast-adjustment-settings",
            DelegatedServingBoundary::AdminAdjustmentSettings,
        ),
        (
            "/api/v1/admin/sites/ballydidean/property-sensor-layout/station",
            DelegatedServingBoundary::AdminPropertySensorUpdate,
        ),
        (
            "/maps/xweather/history/radar/frame",
            DelegatedServingBoundary::XweatherMap,
        ),
        (
            "/__rac/browser-device",
            DelegatedServingBoundary::RemoteAgentsPreview,
        ),
    ];
    // ensure the serving planner never replaces these existing owners
    for (pathname, boundary) in cases {
        assert_eq!(
            plan_public_request("development", "GET", pathname, false),
            PublicServingPlan::Delegated(boundary),
            "{pathname}",
        );
    }
    assert_eq!(
        plan_public_request("development", "POST", "/unknown", false),
        PublicServingPlan::Reject(PublicServingRejection::MethodNotAllowed),
    );
    assert_eq!(
        plan_public_request("development", "POST", "/admin", false),
        PublicServingPlan::Reject(PublicServingRejection::MethodNotAllowed),
    );
    assert_eq!(PublicServingRejection::MethodNotAllowed.status_code(), 405);
    assert_eq!(
        PublicServingRejection::MethodNotAllowed.allow(),
        Some("GET, HEAD")
    );
    assert_eq!(PublicServingRejection::NotFound.body(), "not found\n");
    assert_eq!(
        PublicServingRejection::BadRequest.cache_control(),
        "no-store"
    );
    assert_eq!(
        PublicServingRejection::UnsupportedQuery.body(),
        "unsupported query\n"
    );
    assert_eq!(
        PublicServingRejection::BadRequest.content_type(),
        "text/plain; charset=utf-8"
    );
}

/// Read the cache column from the exact Node API helper output.
fn node_api_cache(pathname: &str) -> String {
    node_oracle(&["api", pathname])
        .split_once('\t')
        .expect("API oracle must include timeout")
        .0
        .to_owned()
}

/// Read the timeout column from the exact Node API helper output.
fn node_api_timeout(pathname: &str) -> u64 {
    node_oracle(&["api", pathname])
        .split_once('\t')
        .expect("API oracle must include timeout")
        .1
        .parse()
        .expect("API timeout must be an integer")
}
