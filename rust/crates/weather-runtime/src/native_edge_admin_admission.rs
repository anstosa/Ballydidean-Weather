//! Native request admission for administrator session, settings, and layout routes.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use hyper::Method;
use hyper::header::HeaderMap;
use subtle::ConstantTimeEq as _;
use url::{Position, Url};
use weather_contracts::JsonValue;
use weather_edge::{
    AdmittedAdminCookieMode, AdmittedAdminCredential, AdmittedAdminLoginPage,
    AdmittedAdminMutation, AdmittedAdminOperator, AdmittedAdminPersistentAuthState,
    AdmittedAdminRouteInput, AdmittedAdminSessionToken, AdmittedPropertySensorMutation,
    DecodedForecastAdjustmentSettings, DecodedPropertySensorLayoutUpdate, EdgeStartupConfiguration,
    WeatherAdminAuthStatePort, WeatherAdminRouteOutcome, WeatherAdminSessionApplicationPort,
};

use crate::{
    NativeEdgeAdminOwner, NativeEdgeHttpRequest, NativeEdgeHttpRequestError,
    NativeEdgeRequestAdmissions, NativeHttpBodyReadError, derive_native_admin_password_verifier,
    native_json::NativeJson, verify_native_admin_credential,
};

const LOGIN_BODY_MAXIMUM_BYTES: usize = 2_048;
const BOOTSTRAP_BODY_MAXIMUM_BYTES: usize = 2_048;
const SETTINGS_BODY_MAXIMUM_BYTES: usize = 1_024;
const LAYOUT_BODY_MAXIMUM_BYTES: usize = 4_096;
const COOKIE_MAXIMUM_CODE_UNITS: usize = 8_192;
const BOOTSTRAP_TOKEN_FILE_MAXIMUM_BYTES: u64 = 4_096;
const ADMIN_SESSION_COOKIE_NAME: &str = "weather_admin_session";
const SETTINGS_PATH: &str = "/api/v1/admin/sites/ballydidean/forecast-adjustment-settings";
const SCORECARD_PATH: &str = "/api/v1/admin/sites/ballydidean/forecast-adjustment-scorecard";
const PROPERTY_SENSOR_UPDATE_PREFIX: &str =
    "/api/v1/admin/sites/ballydidean/property-sensor-layout/";

/// Filesystem-backed admission owner for the configured one-time bootstrap credential.
pub struct NativeEdgeAdminAdmissionOwner {
    bootstrap_token_path: PathBuf,
}

impl NativeEdgeAdminAdmissionOwner {
    /// Retain the already admitted bootstrap token path without reading secret material.
    pub fn from_configuration<Origin>(configuration: &EdgeStartupConfiguration<Origin>) -> Self {
        Self {
            bootstrap_token_path: configuration.admin_bootstrap_token_path.clone(),
        }
    }

    /// Perform route-local synchronous admission before any selected body is consumed.
    pub fn begin(
        &self,
        request: &NativeEdgeHttpRequest,
        admin: &Mutex<NativeEdgeAdminOwner>,
    ) -> Result<NativeEdgeAdminAdmissionOutcome, NativeEdgeAdminAdmissionError> {
        let method = &request.parts().method;
        let headers = &request.parts().headers;
        let pathname = request.target().pathname.as_str();

        // preserve predecessor route order and avoid private work for unrelated requests
        let outcome = match pathname {
            "/admin/login" if method == Method::POST => {
                NativeEdgeAdminAdmissionOutcome::BodyRequired(NativeEdgeAdminBodyPlan::login())
            }
            "/admin/logout" if method == Method::POST => {
                NativeEdgeAdminAdmissionOutcome::Admitted(NativeEdgeAdminAdmission::logout(
                    cookie_mode(headers),
                    read_admin_session_cookie(headers),
                ))
            }
            "/api/v1/admin/bootstrap" if method == Method::POST => {
                NativeEdgeAdminAdmissionOutcome::BodyRequired(NativeEdgeAdminBodyPlan::bootstrap())
            }
            "/admin" | "/admin/" => {
                NativeEdgeAdminAdmissionOutcome::Admitted(NativeEdgeAdminAdmission::page(
                    AdmittedAdminLoginPage {
                        invalid_credentials: request
                            .target()
                            .query
                            .iter()
                            .find_map(|(name, value)| (name == "error").then_some(value))
                            .is_some_and(|value| value == "invalid"),
                    },
                    read_admin_session_cookie(headers),
                ))
            }
            SETTINGS_PATH => {
                let operator = authenticate_admin(headers, admin)?;
                // authenticate every method while deferring only the admitted PUT body
                if method == Method::PUT {
                    require_same_origin(headers)?;
                    NativeEdgeAdminAdmissionOutcome::BodyRequired(
                        NativeEdgeAdminBodyPlan::settings(operator),
                    )
                } else {
                    NativeEdgeAdminAdmissionOutcome::Admitted(NativeEdgeAdminAdmission::operator(
                        operator,
                    ))
                }
            }
            SCORECARD_PATH => NativeEdgeAdminAdmissionOutcome::Admitted(
                NativeEdgeAdminAdmission::operator(authenticate_admin(headers, admin)?),
            ),
            path if path.starts_with(PROPERTY_SENSOR_UPDATE_PREFIX) => {
                let operator = authenticate_admin(headers, admin)?;
                // preserve method rejection after authentication without consuming a body
                if method != Method::PUT {
                    NativeEdgeAdminAdmissionOutcome::Admitted(NativeEdgeAdminAdmission::operator(
                        operator,
                    ))
                } else {
                    require_same_origin(headers)?;
                    let sensor_key =
                        decode_uri_component(&path[PROPERTY_SENSOR_UPDATE_PREFIX.len()..])?;
                    NativeEdgeAdminAdmissionOutcome::BodyRequired(
                        NativeEdgeAdminBodyPlan::property(operator, sensor_key),
                    )
                }
            }
            path if is_static_html_route(path, method) => {
                let admission = match authenticate_optional_admin(headers, admin)? {
                    Some(operator) => NativeEdgeAdminAdmission::operator(operator),
                    None => NativeEdgeAdminAdmission::none(),
                };
                NativeEdgeAdminAdmissionOutcome::Admitted(admission)
            }
            _ => NativeEdgeAdminAdmissionOutcome::Admitted(NativeEdgeAdminAdmission::none()),
        };
        Ok(outcome)
    }

    /// Consume the body selected by `begin` without performing blocking private work.
    pub async fn read_body(
        &self,
        request: &mut NativeEdgeHttpRequest,
        plan: &NativeEdgeAdminBodyPlan,
        body_deadline: Duration,
    ) -> Result<Vec<u8>, NativeEdgeAdminAdmissionError> {
        // bind body consumption to the same admitted route shape
        if !plan.matches(request) {
            return Err(NativeEdgeAdminAdmissionError::Internal);
        }
        read_bounded_body(request, plan.maximum_bytes(), body_deadline).await
    }

    /// Complete one consumed body plan on the bounded blocking route worker.
    pub fn finish(
        &self,
        request: &NativeEdgeHttpRequest,
        plan: NativeEdgeAdminBodyPlan,
        body: Vec<u8>,
        admin: &Mutex<NativeEdgeAdminOwner>,
    ) -> Result<NativeEdgeAdminAdmissionOutcome, NativeEdgeAdminAdmissionError> {
        // refuse a plan moved to a different method or route before using admitted proof
        if !plan.matches(request) {
            return Err(NativeEdgeAdminAdmissionError::Internal);
        }
        let headers = &request.parts().headers;
        let admission = match plan.kind {
            NativeEdgeAdminBodyPlanKind::Login => {
                let password = decode_login_form(headers, &body)?;
                let Some(state) = read_auth_state(admin)? else {
                    return Ok(NativeEdgeAdminAdmissionOutcome::Terminal(
                        WeatherAdminRouteOutcome::LoginInvalidRedirect,
                    ));
                };
                let credential = verify_native_admin_credential(&password, state.verifier())
                    .map_err(|_| NativeEdgeAdminAdmissionError::Internal)?;
                let Some(credential) = credential else {
                    return Ok(NativeEdgeAdminAdmissionOutcome::Terminal(
                        WeatherAdminRouteOutcome::LoginInvalidRedirect,
                    ));
                };
                NativeEdgeAdminAdmission::login(cookie_mode(headers), credential)
            }
            NativeEdgeAdminBodyPlanKind::Bootstrap => {
                let object = decode_json_object(&body)?;
                // preserve already-configured precedence before token or password validation
                if let Some(state) = read_auth_state(admin)? {
                    NativeEdgeAdminAdmission::bootstrap(state)
                } else {
                    admit_bootstrap(&self.bootstrap_token_path, headers, &object)?
                }
            }
            NativeEdgeAdminBodyPlanKind::Settings { operator } => {
                NativeEdgeAdminAdmission::settings_replace(
                    operator,
                    decode_adjustment_settings(&body)?,
                )
            }
            NativeEdgeAdminBodyPlanKind::Property {
                operator,
                sensor_key,
            } => NativeEdgeAdminAdmission::property_replace(
                operator,
                sensor_key,
                decode_property_sensor_update(&body)?,
            ),
        };
        Ok(NativeEdgeAdminAdmissionOutcome::Admitted(admission))
    }
}

/// Opaque request-scoped body selection returned by synchronous pre-admission.
#[derive(Debug)]
pub struct NativeEdgeAdminBodyPlan {
    kind: NativeEdgeAdminBodyPlanKind,
}

#[derive(Debug)]
enum NativeEdgeAdminBodyPlanKind {
    Login,
    Bootstrap,
    Settings {
        operator: AdmittedAdminOperator,
    },
    Property {
        operator: AdmittedAdminOperator,
        sensor_key: String,
    },
}

impl NativeEdgeAdminBodyPlan {
    /// Select the original login form body before password work begins.
    const fn login() -> Self {
        Self {
            kind: NativeEdgeAdminBodyPlanKind::Login,
        }
    }

    /// Select the original bootstrap body before filesystem or state work begins.
    const fn bootstrap() -> Self {
        Self {
            kind: NativeEdgeAdminBodyPlanKind::Bootstrap,
        }
    }

    /// Retain one authenticated operator while the settings body is read.
    const fn settings(operator: AdmittedAdminOperator) -> Self {
        Self {
            kind: NativeEdgeAdminBodyPlanKind::Settings { operator },
        }
    }

    /// Retain one authenticated operator and decoded sensor key while its body is read.
    fn property(operator: AdmittedAdminOperator, sensor_key: String) -> Self {
        Self {
            kind: NativeEdgeAdminBodyPlanKind::Property {
                operator,
                sensor_key,
            },
        }
    }

    /// Return the predecessor's exact finite body bound for this selected route.
    const fn maximum_bytes(&self) -> usize {
        // keep each request body at its route-specific source limit
        match &self.kind {
            NativeEdgeAdminBodyPlanKind::Login => LOGIN_BODY_MAXIMUM_BYTES,
            NativeEdgeAdminBodyPlanKind::Bootstrap => BOOTSTRAP_BODY_MAXIMUM_BYTES,
            NativeEdgeAdminBodyPlanKind::Settings { .. } => SETTINGS_BODY_MAXIMUM_BYTES,
            NativeEdgeAdminBodyPlanKind::Property { .. } => LAYOUT_BODY_MAXIMUM_BYTES,
        }
    }

    /// Verify the plan still belongs to the same method and decoded route shape.
    fn matches(&self, request: &NativeEdgeHttpRequest) -> bool {
        let method = &request.parts().method;
        let path = request.target().pathname.as_str();
        // bind retained admission evidence to the route shape that produced it
        match &self.kind {
            NativeEdgeAdminBodyPlanKind::Login => method == Method::POST && path == "/admin/login",
            NativeEdgeAdminBodyPlanKind::Bootstrap => {
                method == Method::POST && path == "/api/v1/admin/bootstrap"
            }
            NativeEdgeAdminBodyPlanKind::Settings { .. } => {
                method == Method::PUT && path == SETTINGS_PATH
            }
            NativeEdgeAdminBodyPlanKind::Property { sensor_key, .. } => {
                method == Method::PUT
                    && path
                        .strip_prefix(PROPERTY_SENSOR_UPDATE_PREFIX)
                        .and_then(|value| decode_uri_component(value).ok())
                        .is_some_and(|value| value == *sensor_key)
            }
        }
    }
}

/// One body request, terminal route result, or request ready for the existing dispatcher.
#[derive(Debug)]
pub enum NativeEdgeAdminAdmissionOutcome {
    Admitted(NativeEdgeAdminAdmission),
    BodyRequired(NativeEdgeAdminBodyPlan),
    Terminal(WeatherAdminRouteOutcome),
}

/// Bounded admission failures mapped by the outer native wire response owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEdgeAdminAdmissionError {
    InvalidRequest,
    AuthenticationRequired,
    BootstrapUnauthorized,
    SameOriginRequired,
    Internal,
}

impl NativeEdgeAdminAdmissionError {
    /// Return the predecessor's exact HTTP status for this external admission failure.
    pub const fn status_code(self) -> u16 {
        // retain malformed, authentication, origin, and server failure categories
        match self {
            Self::InvalidRequest => 400,
            Self::AuthenticationRequired | Self::BootstrapUnauthorized => 401,
            Self::SameOriginRequired => 403,
            Self::Internal => 500,
        }
    }

    /// Return the predecessor's exact plain-text representation.
    pub const fn body(self) -> &'static [u8] {
        // preserve route-specific unauthorized wording
        match self {
            Self::InvalidRequest => b"invalid request\n",
            Self::AuthenticationRequired => b"authentication required\n",
            Self::BootstrapUnauthorized => b"unauthorized\n",
            Self::SameOriginRequired => b"same-origin request required\n",
            Self::Internal => b"internal server error\n",
        }
    }
}

/// Owned admitted values retained through the synchronous edge dispatch call.
#[derive(Debug)]
pub struct NativeEdgeAdminAdmission {
    kind: NativeEdgeAdminAdmissionKind,
}

#[derive(Debug)]
enum NativeEdgeAdminAdmissionKind {
    None,
    Bootstrap(AdmittedAdminPersistentAuthState),
    Login {
        cookie_mode: AdmittedAdminCookieMode,
        credential: AdmittedAdminCredential,
    },
    Logout {
        cookie_mode: AdmittedAdminCookieMode,
        token: Option<AdmittedAdminSessionToken>,
    },
    Page {
        login: AdmittedAdminLoginPage,
        token: Option<AdmittedAdminSessionToken>,
    },
    Operator(AdmittedAdminOperator),
    SettingsReplace {
        operator: AdmittedAdminOperator,
        input: DecodedForecastAdjustmentSettings,
    },
    PropertyReplace {
        operator: AdmittedAdminOperator,
        sensor_key: String,
        input: DecodedPropertySensorLayoutUpdate,
    },
}

impl NativeEdgeAdminAdmission {
    /// Produce no private input for a route outside this owner.
    const fn none() -> Self {
        Self {
            kind: NativeEdgeAdminAdmissionKind::None,
        }
    }

    /// Retain one verified bootstrap state until dispatch persists it.
    fn bootstrap(state: AdmittedAdminPersistentAuthState) -> Self {
        Self {
            kind: NativeEdgeAdminAdmissionKind::Bootstrap(state),
        }
    }

    /// Retain one verifier-bound login credential and cookie context.
    fn login(cookie_mode: AdmittedAdminCookieMode, credential: AdmittedAdminCredential) -> Self {
        Self {
            kind: NativeEdgeAdminAdmissionKind::Login {
                cookie_mode,
                credential,
            },
        }
    }

    /// Retain one optional syntax-admitted logout token and cookie context.
    fn logout(
        cookie_mode: AdmittedAdminCookieMode,
        token: Option<AdmittedAdminSessionToken>,
    ) -> Self {
        Self {
            kind: NativeEdgeAdminAdmissionKind::Logout { cookie_mode, token },
        }
    }

    /// Retain one optional page token and decoded invalid-login query state.
    fn page(login: AdmittedAdminLoginPage, token: Option<AdmittedAdminSessionToken>) -> Self {
        Self {
            kind: NativeEdgeAdminAdmissionKind::Page { login, token },
        }
    }

    /// Retain one session-authenticated administrator for a private read or method response.
    fn operator(operator: AdmittedAdminOperator) -> Self {
        Self {
            kind: NativeEdgeAdminAdmissionKind::Operator(operator),
        }
    }

    /// Retain one same-origin settings mutation and its bounded decoded object.
    fn settings_replace(
        operator: AdmittedAdminOperator,
        input: DecodedForecastAdjustmentSettings,
    ) -> Self {
        Self {
            kind: NativeEdgeAdminAdmissionKind::SettingsReplace { operator, input },
        }
    }

    /// Retain one same-origin layout mutation, decoded key, and bounded object.
    fn property_replace(
        operator: AdmittedAdminOperator,
        sensor_key: String,
        input: DecodedPropertySensorLayoutUpdate,
    ) -> Self {
        Self {
            kind: NativeEdgeAdminAdmissionKind::PropertyReplace {
                operator,
                sensor_key,
                input,
            },
        }
    }

    /// Borrow route-specific typed values for exactly one existing dispatcher call.
    pub fn request_admissions(&self) -> NativeEdgeRequestAdmissions<'_> {
        let mut admissions = NativeEdgeRequestAdmissions::default();
        // expose only the private fields belonging to this admitted route shape
        match &self.kind {
            NativeEdgeAdminAdmissionKind::None => {}
            NativeEdgeAdminAdmissionKind::Bootstrap(state) => {
                admissions.admin_route_input =
                    Some(AdmittedAdminRouteInput::Bootstrap(state.clone()));
            }
            NativeEdgeAdminAdmissionKind::Login {
                cookie_mode,
                credential,
            } => {
                admissions.admin_route_input = Some(AdmittedAdminRouteInput::Login {
                    cookie_mode: *cookie_mode,
                    credential: credential.clone(),
                });
            }
            NativeEdgeAdminAdmissionKind::Logout { cookie_mode, token } => {
                admissions.admin_route_input = Some(AdmittedAdminRouteInput::Logout {
                    cookie_mode: *cookie_mode,
                    token: token.as_ref(),
                });
            }
            NativeEdgeAdminAdmissionKind::Page { login, token } => {
                admissions.admin_route_input = Some(AdmittedAdminRouteInput::Page {
                    login: *login,
                    token: token.as_ref(),
                });
            }
            NativeEdgeAdminAdmissionKind::Operator(operator) => {
                admissions.admin_operator = Some(operator);
            }
            NativeEdgeAdminAdmissionKind::SettingsReplace { operator, input } => {
                admissions.admin_mutation = Some(AdmittedAdminMutation::admitted(operator));
                admissions.adjustment_settings_body = Some(*input);
            }
            NativeEdgeAdminAdmissionKind::PropertyReplace {
                operator,
                sensor_key,
                input,
            } => {
                admissions.property_sensor_mutation = Some(AdmittedPropertySensorMutation {
                    input: input.clone(),
                    mutation: AdmittedAdminMutation::admitted(operator),
                    sensor_key: sensor_key.clone(),
                });
            }
        }
        admissions
    }
}

/// Consume one body through the shared finite native body owner.
async fn read_bounded_body(
    request: &mut NativeEdgeHttpRequest,
    maximum: usize,
    deadline: Duration,
) -> Result<Vec<u8>, NativeEdgeAdminAdmissionError> {
    // refuse a disabled deadline instead of retaining an unbounded request
    if deadline.is_zero() {
        return Err(NativeEdgeAdminAdmissionError::Internal);
    }
    request
        .read_body(maximum, deadline)
        .await
        .map_err(|error| match error {
            NativeEdgeHttpRequestError::Body(NativeHttpBodyReadError::TooLarge) => {
                NativeEdgeAdminAdmissionError::InvalidRequest
            }
            NativeEdgeHttpRequestError::BodyConsumed
            | NativeEdgeHttpRequestError::Target
            | NativeEdgeHttpRequestError::Body(NativeHttpBodyReadError::Deadline)
            | NativeEdgeHttpRequestError::Body(NativeHttpBodyReadError::Transport(_)) => {
                NativeEdgeAdminAdmissionError::Internal
            }
        })
}

/// Decode one exact URL-encoded login form using URLSearchParams semantics.
fn decode_login_form(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<String, NativeEdgeAdminAdmissionError> {
    let content_type = joined_header(headers, "content-type", ", ").unwrap_or_default();
    let content_type = content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    // require the browser's native form representation before parsing fields
    if content_type != "application/x-www-form-urlencoded" {
        return Err(NativeEdgeAdminAdmissionError::InvalidRequest);
    }
    let mut password = None;
    // require exactly one password field and reject every extension field
    for (name, value) in url::form_urlencoded::parse(body) {
        if name != "password" || password.is_some() {
            return Err(NativeEdgeAdminAdmissionError::InvalidRequest);
        }
        password = Some(value.into_owned());
    }
    password.ok_or(NativeEdgeAdminAdmissionError::InvalidRequest)
}

/// Decode a complete JSON document and require its object representation.
fn decode_json_object(
    bytes: &[u8],
) -> Result<Vec<(Vec<u16>, JsonValue)>, NativeEdgeAdminAdmissionError> {
    let JsonValue::Object(fields) =
        NativeJson::decode(bytes).ok_or(NativeEdgeAdminAdmissionError::InvalidRequest)?
    else {
        return Err(NativeEdgeAdminAdmissionError::InvalidRequest);
    };
    Ok(fields)
}

/// Admit first-run bootstrap token and password material without retaining plaintext.
fn admit_bootstrap(
    token_path: &Path,
    headers: &HeaderMap,
    object: &[(Vec<u16>, JsonValue)],
) -> Result<NativeEdgeAdminAdmission, NativeEdgeAdminAdmissionError> {
    let Some(expected) = read_optional_bootstrap_token(token_path)? else {
        return Err(NativeEdgeAdminAdmissionError::BootstrapUnauthorized);
    };
    let provided = joined_header(headers, "x-weather-admin-bootstrap", ", ")
        .ok_or(NativeEdgeAdminAdmissionError::InvalidRequest)?;
    // validate submitted secret text before performing the constant-time comparison
    if !valid_secret(&provided, 32, 256) {
        return Err(NativeEdgeAdminAdmissionError::InvalidRequest);
    }
    if !secure_text_equal(expected.as_bytes(), provided.as_bytes()) {
        return Err(NativeEdgeAdminAdmissionError::BootstrapUnauthorized);
    }
    let password = object_text(object, "password")
        .filter(|password| valid_secret(password, 8, 256))
        .ok_or(NativeEdgeAdminAdmissionError::InvalidRequest)?;
    let verifier = derive_native_admin_password_verifier(&password)
        .map_err(|_| NativeEdgeAdminAdmissionError::Internal)?;
    let state = crate::admit_native_admin_persistent_auth_state(verifier)
        .map_err(|_| NativeEdgeAdminAdmissionError::Internal)?;
    Ok(NativeEdgeAdminAdmission::bootstrap(state))
}

/// Read one optional trimmed bootstrap token under a finite configured-file bound.
fn read_optional_bootstrap_token(
    path: &Path,
) -> Result<Option<String>, NativeEdgeAdminAdmissionError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(NativeEdgeAdminAdmissionError::Internal),
    };
    let mut bytes = Vec::new();
    file.take(BOOTSTRAP_TOKEN_FILE_MAXIMUM_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NativeEdgeAdminAdmissionError::Internal)?;
    // refuse a malformed installed secret file without allocating its full contents
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > BOOTSTRAP_TOKEN_FILE_MAXIMUM_BYTES {
        return Err(NativeEdgeAdminAdmissionError::Internal);
    }
    Ok(Some(String::from_utf8_lossy(&bytes).trim().to_owned()))
}

/// Read one optional persistent state without retaining the administrator mutex.
fn read_auth_state(
    admin: &Mutex<NativeEdgeAdminOwner>,
) -> Result<Option<AdmittedAdminPersistentAuthState>, NativeEdgeAdminAdmissionError> {
    admin
        .lock()
        .map_err(|_| NativeEdgeAdminAdmissionError::Internal)?
        .auth_state_mut()
        .read_auth_state()
        .map_err(|_| NativeEdgeAdminAdmissionError::Internal)
}

/// Authenticate one syntax-admitted session and return only opaque operator evidence.
fn authenticate_admin(
    headers: &HeaderMap,
    admin: &Mutex<NativeEdgeAdminOwner>,
) -> Result<AdmittedAdminOperator, NativeEdgeAdminAdmissionError> {
    let token = read_admin_session_cookie(headers)
        .ok_or(NativeEdgeAdminAdmissionError::AuthenticationRequired)?;
    let authenticated = admin
        .lock()
        .map_err(|_| NativeEdgeAdminAdmissionError::Internal)?
        .session_application()
        .authenticate(&token)
        .map_err(|_| NativeEdgeAdminAdmissionError::Internal)?;
    // return no operator identity unless the retained session owner proves it
    if !authenticated {
        return Err(NativeEdgeAdminAdmissionError::AuthenticationRequired);
    }
    Ok(AdmittedAdminOperator::admitted("admin"))
}

/// Authenticate an optional static-template session without making public pages private.
fn authenticate_optional_admin(
    headers: &HeaderMap,
    admin: &Mutex<NativeEdgeAdminOwner>,
) -> Result<Option<AdmittedAdminOperator>, NativeEdgeAdminAdmissionError> {
    let Some(token) = read_admin_session_cookie(headers) else {
        return Ok(None);
    };
    let authenticated = admin
        .lock()
        .map_err(|_| NativeEdgeAdminAdmissionError::Internal)?
        .session_application()
        .authenticate(&token)
        .map_err(|_| NativeEdgeAdminAdmissionError::Internal)?;
    // retain the administrator shell only for an active server-owned session
    if authenticated {
        Ok(Some(AdmittedAdminOperator::admitted("admin")))
    } else {
        Ok(None)
    }
}

/// Select only allowlisted HTML templates whose representation depends on session state.
fn is_static_html_route(path: &str, method: &Method) -> bool {
    // avoid session storage work for methods and assets that cannot render HTML
    if method != Method::GET && method != Method::HEAD {
        return false;
    }
    matches!(
        path,
        "/" | "/index.html"
            | "/logs"
            | "/logs/"
            | "/map"
            | "/map/"
            | "/forecast"
            | "/forecast/"
            | "/trends"
            | "/trends/"
            | "/settings"
            | "/settings/"
            | "/privacy"
            | "/privacy/"
    )
}

/// Read exactly one bounded administrator cookie with the original token grammar.
fn read_admin_session_cookie(headers: &HeaderMap) -> Option<AdmittedAdminSessionToken> {
    let header = joined_header(headers, "cookie", "; ")?;
    // reject missing or oversized cookie collections before scanning pairs
    if header.encode_utf16().count() > COOKIE_MAXIMUM_CODE_UNITS {
        return None;
    }
    let prefix = format!("{ADMIN_SESSION_COOKIE_NAME}=");
    let mut token = None;
    // retain only one exact administrator cookie pair
    for entry in header.split(';') {
        let entry = entry.trim();
        let Some(value) = entry.strip_prefix(&prefix) else {
            continue;
        };
        if token.is_some() || !valid_session_token(value) {
            return None;
        }
        token = Some(AdmittedAdminSessionToken::admitted(value));
    }
    token
}

/// Select the exact cookie attribute mode from the trusted proxy protocol header.
fn cookie_mode(headers: &HeaderMap) -> AdmittedAdminCookieMode {
    let secure = joined_header(headers, "x-forwarded-proto", ", ")
        .and_then(|value| value.split(',').next().map(str::trim).map(str::to_owned))
        .is_some_and(|value| value == "https");
    // preserve iframe-compatible secure cookies only for an HTTPS transport context
    if secure {
        AdmittedAdminCookieMode::SecurePartitioned
    } else {
        AdmittedAdminCookieMode::SameSiteLax
    }
}

/// Require one concrete browser origin equal to the native request's reconstructed origin.
fn require_same_origin(headers: &HeaderMap) -> Result<(), NativeEdgeAdminAdmissionError> {
    let origin = joined_header(headers, "origin", ", ")
        .ok_or(NativeEdgeAdminAdmissionError::SameOriginRequired)?;
    let host =
        exact_header(headers, "host").ok_or(NativeEdgeAdminAdmissionError::SameOriginRequired)?;
    let scheme = if cookie_mode(headers) == AdmittedAdminCookieMode::SecurePartitioned {
        "https"
    } else {
        "http"
    };
    let expected = Url::parse(&format!("{scheme}://{host}"))
        .map_err(|_| NativeEdgeAdminAdmissionError::SameOriginRequired)?;
    let submitted =
        Url::parse(&origin).map_err(|_| NativeEdgeAdminAdmissionError::SameOriginRequired)?;
    let expected_host = &expected[Position::BeforeHost..Position::AfterPort];
    let expected_origin = expected.origin().ascii_serialization();
    let submitted_origin = submitted.origin().ascii_serialization();
    // require canonical host and origin spellings rather than URL-equivalent attacker text
    if expected_host != host || submitted_origin != expected_origin || origin != submitted_origin {
        return Err(NativeEdgeAdminAdmissionError::SameOriginRequired);
    }
    Ok(())
}

/// Decode the settings object's shape while leaving exact validation to its policy owner.
fn decode_adjustment_settings(
    bytes: &[u8],
) -> Result<DecodedForecastAdjustmentSettings, NativeEdgeAdminAdmissionError> {
    let fields = decode_json_object(bytes)?;
    Ok(DecodedForecastAdjustmentSettings {
        field_count: fields.len(),
        version: object_number(&fields, "version").and_then(json_u64),
        temperature: object_boolean(&fields, "temperature"),
        wind: object_boolean(&fields, "wind"),
        rain: object_boolean(&fields, "rain"),
    })
}

/// Decode eligible sensor fields while leaving geographic validation to its policy owner.
fn decode_property_sensor_update(
    bytes: &[u8],
) -> Result<DecodedPropertySensorLayoutUpdate, NativeEdgeAdminAdmissionError> {
    let fields = decode_json_object(bytes)?;
    Ok(DecodedPropertySensorLayoutUpdate {
        display_name: object_text(&fields, "displayName"),
        icon: object_text(&fields, "icon"),
        latitude: object_number(&fields, "latitude").copied(),
        longitude: object_number(&fields, "longitude").copied(),
    })
}

/// Decode JavaScript's strict decodeURIComponent byte grammar without form `+` handling.
fn decode_uri_component(value: &str) -> Result<String, NativeEdgeAdminAdmissionError> {
    let input = value.as_bytes();
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    // decode every complete percent triplet and retain all other literal UTF-8 bytes
    while index < input.len() {
        if input[index] == b'%' {
            let high = input
                .get(index + 1)
                .and_then(|value| hex_nibble(*value))
                .ok_or(NativeEdgeAdminAdmissionError::InvalidRequest)?;
            let low = input
                .get(index + 2)
                .and_then(|value| hex_nibble(*value))
                .ok_or(NativeEdgeAdminAdmissionError::InvalidRequest)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| NativeEdgeAdminAdmissionError::InvalidRequest)
}

/// Decode one hexadecimal percent-escape digit.
const fn hex_nibble(value: u8) -> Option<u8> {
    // preserve both decodeURIComponent hexadecimal alphabets
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// Join raw HTTP fields using Node's route-specific visible header convention.
fn joined_header(headers: &HeaderMap, name: &str, separator: &str) -> Option<String> {
    let mut values = headers.get_all(name).iter();
    let first = latin1(values.next()?.as_bytes());
    Some(values.fold(first, |mut output, value| {
        output.push_str(separator);
        output.push_str(&latin1(value.as_bytes()));
        output
    }))
}

/// Admit one single header where the predecessor requires a concrete scalar.
fn exact_header(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut values = headers.get_all(name).iter();
    let value = latin1(values.next()?.as_bytes());
    // reject duplicates rather than choosing one identity-bearing field
    if values.next().is_some() {
        return None;
    }
    Some(value)
}

/// Preserve Node's Latin-1 request-header string projection.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

/// Find one decoded JSON member by exact UTF-16 key identity.
fn object_field<'a>(fields: &'a [(Vec<u16>, JsonValue)], name: &str) -> Option<&'a JsonValue> {
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
}

/// Decode one JSON string with JavaScript's lossy UTF-8 boundary for lone surrogates.
fn object_text(fields: &[(Vec<u16>, JsonValue)], name: &str) -> Option<String> {
    let JsonValue::String(value) = object_field(fields, name)? else {
        return None;
    };
    Some(String::from_utf16_lossy(value))
}

/// Decode one JSON boolean without truthy coercion.
fn object_boolean(fields: &[(Vec<u16>, JsonValue)], name: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = object_field(fields, name)? else {
        return None;
    };
    Some(*value)
}

/// Borrow one JSON number without string coercion.
fn object_number<'a>(fields: &'a [(Vec<u16>, JsonValue)], name: &str) -> Option<&'a f64> {
    let JsonValue::Number(value) = object_field(fields, name)? else {
        return None;
    };
    Some(value)
}

/// Retain only exact nonnegative JavaScript-safe integer fields.
fn json_u64(value: &f64) -> Option<u64> {
    (value.is_finite()
        && *value >= 0.0
        && *value <= 9_007_199_254_740_991.0
        && value.fract() == 0.0)
        .then_some(*value as u64)
}

/// Preserve the predecessor's bounded UTF-16 secret and control-character grammar.
fn valid_secret(value: &str, minimum: usize, maximum: usize) -> bool {
    let length = value.encode_utf16().count();
    (minimum..=maximum).contains(&length)
        && !value
            .chars()
            .any(|character| matches!(character, '\u{0000}'..='\u{001f}' | '\u{007f}'))
}

/// Compare secret bytes without a length-dependent successful path.
fn secure_text_equal(expected: &[u8], provided: &[u8]) -> bool {
    // normalize unequal lengths through one equal-length constant-time comparison
    if expected.len() != provided.len() {
        let padding = vec![0_u8; expected.len()];
        let _ = expected.ct_eq(&padding);
        return false;
    }
    bool::from(expected.ct_eq(provided))
}

/// Recognize only the predecessor's 32-byte unpadded base64url token spelling.
fn valid_session_token(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use hyper::header::CONTENT_TYPE;

    use super::*;

    /// Decode one ordinary browser login form through the production form owner.
    #[test]
    fn decodes_native_admin_login_form() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            "application/x-www-form-urlencoded; charset=UTF-8"
                .parse()
                .unwrap(),
        );

        assert_eq!(
            decode_login_form(&headers, b"password=farm+weather%21").unwrap(),
            "farm weather!"
        );
    }

    /// Project one ordinary complete adjustment settings object into typed fields.
    #[test]
    fn decodes_native_adjustment_settings_object() {
        assert_eq!(
            decode_adjustment_settings(
                br#"{"version":1,"temperature":true,"wind":false,"rain":true}"#,
            )
            .unwrap(),
            DecodedForecastAdjustmentSettings {
                field_count: 4,
                version: Some(1),
                temperature: Some(true),
                wind: Some(false),
                rain: Some(true),
            }
        );
    }

    /// Decode the layout route's percent-encoded stable sensor identity.
    #[test]
    fn decodes_native_property_sensor_route_key() {
        assert_eq!(
            decode_uri_component("garden%2Dtemperature").unwrap(),
            "garden-temperature"
        );
    }
}
