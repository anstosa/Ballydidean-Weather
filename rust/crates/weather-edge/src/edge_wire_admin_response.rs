//! Materialize admitted administrator outcomes without serializing cookie policy.

use crate::edge_wire_response::EdgeWireResponse;
use crate::{AdmittedAdminCookieMode, WeatherAdminRouteOutcome, WeatherAdminSessionResponse};

const ADMIN_ALREADY_CONFIGURED_BODY: &[u8] = b"admin access is already configured\n";
const ADMIN_LOCATION: &str = "/admin";
const BOOTSTRAP_CONFIGURED_JSON: &[u8] = b"{\"configured\":true}\n";
const INTERNAL_SERVER_ERROR_BODY: &[u8] = b"internal server error\n";
const INVALID_LOGIN_LOCATION: &str = "/admin?error=invalid";
const METHOD_NOT_ALLOWED_BODY: &[u8] = b"method not allowed\n";

/// Opaque administrator cookie action retained for the approved transport policy owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeWireAdminCookie {
    Issue {
        cookie_mode: AdmittedAdminCookieMode,
        session: WeatherAdminSessionResponse,
    },
    Clear {
        cookie_mode: AdmittedAdminCookieMode,
    },
}

/// Materialize one admitted administrator response and retain cookie actions as typed values.
pub(crate) fn materialize_admin_response(outcome: WeatherAdminRouteOutcome) -> EdgeWireResponse {
    let status_code = outcome.status_code();
    let cache_control = outcome.cache_control();
    match outcome {
        WeatherAdminRouteOutcome::BootstrapConfigured => {
            EdgeWireResponse::fixed_json(status_code, BOOTSTRAP_CONFIGURED_JSON, cache_control)
        }
        WeatherAdminRouteOutcome::BootstrapAlreadyConfigured => EdgeWireResponse::text(
            status_code,
            ADMIN_ALREADY_CONFIGURED_BODY,
            cache_control,
            None,
        ),
        WeatherAdminRouteOutcome::LoginRedirect {
            cookie_mode,
            session,
        } => {
            let mut response =
                EdgeWireResponse::redirect(status_code, ADMIN_LOCATION, cache_control);
            response.admin_cookie = Some(EdgeWireAdminCookie::Issue {
                cookie_mode,
                session,
            });
            response
        }
        WeatherAdminRouteOutcome::LoginInvalidRedirect => {
            EdgeWireResponse::redirect(status_code, INVALID_LOGIN_LOCATION, cache_control)
        }
        WeatherAdminRouteOutcome::LogoutRedirect { cookie_mode } => {
            let mut response =
                EdgeWireResponse::redirect(status_code, ADMIN_LOCATION, cache_control);
            response.admin_cookie = Some(EdgeWireAdminCookie::Clear { cookie_mode });
            response
        }
        WeatherAdminRouteOutcome::MethodNotAllowed { allow } => EdgeWireResponse::text(
            status_code,
            METHOD_NOT_ALLOWED_BODY,
            cache_control,
            Some(allow.header_value()),
        ),
        WeatherAdminRouteOutcome::StoreFailure => {
            EdgeWireResponse::text(status_code, INTERNAL_SERVER_ERROR_BODY, cache_control, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{AdmittedAdminSessionToken, WeatherAdminRouteAllow};

    use super::*;

    /// Prove bootstrap success uses the fixed newline JSON representation and exact length.
    #[test]
    fn materializes_bootstrap_success_and_existing_configuration() {
        let response = materialize_admin_response(WeatherAdminRouteOutcome::BootstrapConfigured);
        assert_eq!(response.status_code, 201);
        assert_eq!(response.headers.cache_control, "no-store");
        assert_eq!(
            response.headers.content_type.as_deref(),
            Some("application/json; charset=utf-8")
        );
        assert_eq!(
            response.headers.content_length,
            BOOTSTRAP_CONFIGURED_JSON.len()
        );
        assert_eq!(response.body.as_bytes(), BOOTSTRAP_CONFIGURED_JSON);
        assert_eq!(response.admin_cookie, None);

        let response =
            materialize_admin_response(WeatherAdminRouteOutcome::BootstrapAlreadyConfigured);
        assert_eq!(response.status_code, 409);
        assert_eq!(
            response.headers.content_length,
            ADMIN_ALREADY_CONFIGURED_BODY.len()
        );
        assert_eq!(response.body.as_bytes(), ADMIN_ALREADY_CONFIGURED_BODY);
        assert_eq!(
            response.headers.content_type.as_deref(),
            Some("text/plain; charset=utf-8")
        );
    }

    /// Prove successful login redirects while retaining the opaque typed issue action.
    #[test]
    fn materializes_login_redirect_and_cookie_issue() {
        let response = materialize_admin_response(WeatherAdminRouteOutcome::LoginRedirect {
            cookie_mode: AdmittedAdminCookieMode::SecurePartitioned,
            session: WeatherAdminSessionResponse {
                maximum_age_seconds: 43_200,
                token: AdmittedAdminSessionToken::admitted("opaque-session"),
            },
        });

        assert_eq!(response.status_code, 303);
        assert_eq!(response.headers.location.as_deref(), Some("/admin"));
        assert_eq!(response.headers.content_length, 0);
        assert_eq!(response.headers.content_type, None);
        assert!(response.body.is_empty());
        match response.admin_cookie {
            Some(EdgeWireAdminCookie::Issue {
                cookie_mode,
                session,
            }) => {
                assert_eq!(cookie_mode, AdmittedAdminCookieMode::SecurePartitioned);
                assert_eq!(session.maximum_age_seconds, 43_200);
                assert_eq!(session.token.expose(), "opaque-session");
            }
            _ => panic!("expected typed issue action"),
        }
    }

    /// Prove invalid login redirects without a cookie and logout requests a typed clear.
    #[test]
    fn materializes_invalid_login_and_logout_redirects() {
        let invalid = materialize_admin_response(WeatherAdminRouteOutcome::LoginInvalidRedirect);
        assert_eq!(invalid.status_code, 303);
        assert_eq!(
            invalid.headers.location.as_deref(),
            Some("/admin?error=invalid")
        );
        assert_eq!(invalid.admin_cookie, None);

        let logout = materialize_admin_response(WeatherAdminRouteOutcome::LogoutRedirect {
            cookie_mode: AdmittedAdminCookieMode::SameSiteLax,
        });
        assert_eq!(logout.status_code, 303);
        assert_eq!(logout.headers.location.as_deref(), Some("/admin"));
        assert_eq!(
            logout.admin_cookie,
            Some(EdgeWireAdminCookie::Clear {
                cookie_mode: AdmittedAdminCookieMode::SameSiteLax,
            })
        );
    }

    /// Prove method and store failures retain their ordinary text response metadata.
    #[test]
    fn materializes_admin_text_failures() {
        let method = materialize_admin_response(WeatherAdminRouteOutcome::MethodNotAllowed {
            allow: WeatherAdminRouteAllow::Post,
        });
        assert_eq!(method.status_code, 405);
        assert_eq!(method.headers.allow, Some("POST"));
        assert_eq!(method.body.as_bytes(), METHOD_NOT_ALLOWED_BODY);
        assert_eq!(method.headers.content_length, METHOD_NOT_ALLOWED_BODY.len());

        let failure = materialize_admin_response(WeatherAdminRouteOutcome::StoreFailure);
        assert_eq!(failure.status_code, 500);
        assert_eq!(failure.headers.allow, None);
        assert_eq!(failure.body.as_bytes(), INTERNAL_SERVER_ERROR_BODY);
        assert_eq!(
            failure.headers.content_length,
            INTERNAL_SERVER_ERROR_BODY.len()
        );
    }
}
