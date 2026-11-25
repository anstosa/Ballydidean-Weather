//! Materialize the existing edge response policy as Hyper response headers.

use hyper::Response;
use hyper::header::{
    ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, RETRY_AFTER, SET_COOKIE, VARY,
};
use weather_edge::{
    AdmittedAdminCookieMode, EdgeWireAdminCookie, EdgeWireHeaders, PublicServingHeaderProfile,
    WeatherAdminSessionResponse,
};

const CONTENT_SECURITY_POLICY: &str = concat!(
    "default-src 'self'; base-uri 'none'; connect-src 'self'; font-src 'self'; ",
    "frame-ancestors *; form-action 'self'; img-src 'self' blob: data: ",
    "https://tile.openstreetmap.org https://basemap.nationalmap.gov ",
    "https://imagery.nationalmap.gov; object-src 'none'; script-src 'self'; ",
    "style-src 'self'; worker-src 'self'",
);
const ANALYTICS_CONTENT_SECURITY_POLICY: &str = concat!(
    "default-src 'self'; base-uri 'none'; connect-src 'self' ",
    "https://www.googletagmanager.com https://*.google-analytics.com https://*.google.com; ",
    "font-src 'self'; frame-ancestors *; form-action 'self'; img-src 'self' blob: data: ",
    "https://tile.openstreetmap.org https://basemap.nationalmap.gov ",
    "https://imagery.nationalmap.gov https://www.googletagmanager.com ",
    "https://*.google-analytics.com; object-src 'none'; script-src 'self' ",
    "https://www.googletagmanager.com; style-src 'self'; worker-src 'self'",
);
const ADMIN_SESSION_COOKIE_NAME: &str = "weather_admin_session";

/// bind one typed edge response to the original Node header policy.
pub fn native_edge_http_head(
    status: u16,
    profile: PublicServingHeaderProfile,
    headers: &EdgeWireHeaders,
    admin_cookie: Option<&EdgeWireAdminCookie>,
) -> hyper::http::response::Builder {
    let (content_security_policy, cross_origin_resource_policy) = match profile {
        PublicServingHeaderProfile::Standard => (CONTENT_SECURITY_POLICY, "same-origin"),
        PublicServingHeaderProfile::Html {
            analytics_enabled: false,
        } => (CONTENT_SECURITY_POLICY, "cross-origin"),
        PublicServingHeaderProfile::Html {
            analytics_enabled: true,
        } => (ANALYTICS_CONTENT_SECURITY_POLICY, "cross-origin"),
    };
    let mut response = Response::builder()
        .status(status)
        .header("content-security-policy", content_security_policy)
        .header("cross-origin-resource-policy", cross_origin_resource_policy)
        .header("referrer-policy", "strict-origin-when-cross-origin")
        .header("x-content-type-options", "nosniff")
        .header(CACHE_CONTROL, headers.cache_control.as_ref())
        .header(CONTENT_LENGTH, headers.content_length.to_string());
    // retain every optional application header without manufacturing absent metadata
    // preserve the route-specific method advertisement
    if let Some(value) = headers.allow {
        response = response.header(ALLOW, value);
    }
    // preserve an intentionally absent redirect content type
    if let Some(value) = headers.content_type.as_deref() {
        response = response.header(CONTENT_TYPE, value);
    }
    // preserve only an application-selected redirect target
    if let Some(value) = headers.location.as_deref() {
        response = response.header(LOCATION, value);
    }
    // preserve bounded retry guidance from its owning application
    if let Some(value) = headers.retry_after.as_deref() {
        response = response.header(RETRY_AFTER, value);
    }
    // expose tile age only for successful cached provider images
    if let Some(value) = headers.tile_age_seconds {
        response = response.header("x-weather-tile-age", value.to_string());
    }
    // expose only the closed hit-or-miss tile outcome
    if let Some(value) = headers.tile_cache {
        response = response.header("x-weather-tile-cache", value.wire_value());
    }
    // preserve representation selection metadata without synthesizing it
    if let Some(value) = headers.vary {
        response = response.header(VARY, value);
    }
    // serialize only an already-admitted administrator cookie action
    if let Some(cookie) = admin_cookie {
        response = response.header(SET_COOKIE, admin_cookie_value(cookie));
    }
    response
}

/// serialize one admitted cookie action in the original source attribute order.
fn admin_cookie_value(cookie: &EdgeWireAdminCookie) -> String {
    match cookie {
        EdgeWireAdminCookie::Issue {
            cookie_mode,
            session,
        } => issue_cookie(*cookie_mode, session),
        EdgeWireAdminCookie::Clear { cookie_mode } => cookie_value(*cookie_mode, "", 0),
    }
}

/// retain the admitted opaque token and exact session lifetime.
fn issue_cookie(
    cookie_mode: AdmittedAdminCookieMode,
    session: &WeatherAdminSessionResponse,
) -> String {
    cookie_value(
        cookie_mode,
        session.token.expose(),
        session.maximum_age_seconds,
    )
}

/// append context attributes selected before response materialization.
fn cookie_value(
    cookie_mode: AdmittedAdminCookieMode,
    token: &str,
    maximum_age_seconds: u64,
) -> String {
    let context = match cookie_mode {
        AdmittedAdminCookieMode::SameSiteLax => "SameSite=Lax",
        AdmittedAdminCookieMode::SecurePartitioned => "SameSite=None; Secure; Partitioned",
    };
    format!(
        "{ADMIN_SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; Max-Age={maximum_age_seconds}; {context}"
    )
}
