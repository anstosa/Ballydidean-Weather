use crate::{
    ApiContentTypePlan, ApiProxyServingPlan, ApiUpstreamMethod, DelegatedServingBoundary,
    PublicServingPlan, StaticAssetRoot, StaticAssetServingPlan, plan_public_request,
};

const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";
const JSON_ACCEPT: &str = "application/json";

/// response projection retained by the existing API response owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicApiProjection {
    None,
    Forecast,
    RainCollection,
}

/// one complete source asset admitted by the selected asset root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedPublicAssetBytes(Vec<u8>);

impl AdmittedPublicAssetBytes {
    /// admit one complete public asset or rendered template.
    pub fn admitted(content: impl Into<Vec<u8>>) -> Self {
        Self(content.into())
    }

    /// borrow the complete admitted asset bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// exact allowlisted asset loading retained outside request policy.
pub trait PublicStaticAssetRead {
    type Error;

    /// read one complete asset from its already-selected root and relative path.
    fn read_asset(
        &self,
        root: StaticAssetRoot,
        relative_path: &str,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error>;
}

/// admitted template context selected by the public serving plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicStaticTemplateRenderRequest<'a> {
    pub administrator: bool,
    pub analytics_enabled: bool,
    pub pathname: &'a str,
}

/// release, digest, preload-time, and HTML rendering retained by its existing owner.
pub trait PublicStaticTemplateRenderer {
    type Error;

    /// render one source template using an admitted runtime context.
    fn render_template(
        &self,
        source: &AdmittedPublicAssetBytes,
        request: PublicStaticTemplateRenderRequest<'_>,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error>;
}

/// one admitted upstream response after any selected API projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedPublicApiUpstreamResponse {
    pub body: Vec<u8>,
    pub content_length: Option<usize>,
    pub content_type: Option<String>,
    pub status_code: u16,
    pub successful: bool,
}

/// one exact API fetch selected by the existing serving plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicApiProxyClientRequest<'a> {
    pub accept: &'a str,
    pub maximum_response_bytes: usize,
    pub pathname: &'a str,
    pub projection: PublicApiProjection,
    pub search: &'a str,
    pub timeout_ms: u64,
    pub upstream_method: ApiUpstreamMethod,
}

/// bounded upstream transport and response projection retained outside request policy.
pub trait PublicApiProxyClient {
    type Error;

    /// bound the original upstream body before projecting the selected representation.
    fn fetch_api(
        &self,
        request: PublicApiProxyClientRequest<'_>,
    ) -> Result<AdmittedPublicApiUpstreamResponse, Self::Error>;
}

/// one parsed public request with its admitted same-origin query spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicServingApplicationRequest<'a> {
    pub accept: Option<&'a str>,
    pub method: &'a str,
    pub pathname: &'a str,
    pub release: &'a str,
    pub search: &'a str,
}

/// one authenticated administrator page request after session admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminPageApplicationRequest<'a> {
    pub omit_body: bool,
    pub pathname: &'a str,
}

/// one normal administrator login representation request after query admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminLoginPageApplicationRequest {
    pub invalid_credentials: bool,
    pub omit_body: bool,
}

/// admitted login template values selected by the existing representation owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminLoginPageRenderRequest {
    pub invalid_credentials: bool,
}

/// administrator login template substitution retained outside route policy.
pub trait AdminLoginPageRenderer {
    type Error;

    /// render one admitted login document from its source template.
    fn render_login_page(
        &self,
        source: &AdmittedPublicAssetBytes,
        request: AdminLoginPageRenderRequest,
    ) -> Result<AdmittedPublicAssetBytes, Self::Error>;
}

/// one ordinary body retained for the owning protocol adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicServingApplicationBody {
    Bytes(Option<Vec<u8>>),
    Text(&'static [u8]),
}

/// response header policy selected by the same metadata that owns document rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicServingHeaderProfile {
    Standard,
    Html { analytics_enabled: bool },
}

/// exact ordinary headers selected by the static or API application path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicServingApplicationHeaders {
    pub allow: Option<&'static str>,
    pub cache_control: String,
    pub content_length: usize,
    pub content_type: String,
    pub vary: Option<&'static str>,
}

/// one complete static or API application response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicServingApplicationResponse {
    pub body: PublicServingApplicationBody,
    pub header_profile: PublicServingHeaderProfile,
    pub headers: PublicServingApplicationHeaders,
    pub status_code: u16,
    pub track_successful_forecast_response: bool,
}

/// existing owner selected for a route outside static and ordinary API handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicServingApplicationBoundary {
    Delegated(DelegatedServingBoundary),
    RecordedForecast,
    WidgetForecast,
}

/// handled response or one existing application boundary left untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicServingApplicationOutcome {
    Handled(PublicServingApplicationResponse),
    Boundary(PublicServingApplicationBoundary),
}

/// serve allowlisted assets and the ordinary public API proxy through typed ports.
pub fn handle_public_static_and_api<Assets, Renderer, Api>(
    request: PublicServingApplicationRequest<'_>,
    assets: &Assets,
    renderer: &Renderer,
    api: &Api,
) -> PublicServingApplicationOutcome
where
    Assets: PublicStaticAssetRead,
    Renderer: PublicStaticTemplateRenderer,
    Api: PublicApiProxyClient,
{
    let plan = plan_public_request(
        request.release,
        request.method,
        request.pathname,
        !request.search.is_empty(),
    );

    // execute only static, ordinary API, and shared public rejection plans
    match plan {
        PublicServingPlan::StaticAsset(plan) => PublicServingApplicationOutcome::Handled(
            serve_static_asset(request.pathname, plan, false, assets, renderer),
        ),
        PublicServingPlan::ApiProxy(plan) => {
            PublicServingApplicationOutcome::Handled(serve_api_proxy(request, plan, api))
        }
        PublicServingPlan::Reject(rejection) => {
            PublicServingApplicationOutcome::Handled(public_text_response(
                rejection.status_code(),
                rejection.body().as_bytes(),
                rejection.allow(),
            ))
        }
        PublicServingPlan::Widget(_) => PublicServingApplicationOutcome::Boundary(
            PublicServingApplicationBoundary::WidgetForecast,
        ),
        PublicServingPlan::RecordedForecast(_) => PublicServingApplicationOutcome::Boundary(
            PublicServingApplicationBoundary::RecordedForecast,
        ),
        PublicServingPlan::Delegated(boundary) => PublicServingApplicationOutcome::Boundary(
            PublicServingApplicationBoundary::Delegated(boundary),
        ),
    }
}

/// render the authenticated administrator shell through the existing template owner.
pub fn handle_admin_page_application<Assets, Renderer>(
    request: AdminPageApplicationRequest<'_>,
    assets: &Assets,
    renderer: &Renderer,
) -> PublicServingApplicationResponse
where
    Assets: PublicStaticAssetRead,
    Renderer: PublicStaticTemplateRenderer,
{
    // preserve the private no-store administrator representation metadata
    serve_static_asset(
        request.pathname,
        StaticAssetServingPlan {
            root: StaticAssetRoot::Public,
            relative_path: "index.html",
            template: true,
            analytics_allowed: false,
            omit_body: request.omit_body,
            cache_control: "private, no-store",
            content_type: "text/html; charset=utf-8",
        },
        true,
        assets,
        renderer,
    )
}

/// render the administrator sign-in document through its existing template owner.
pub fn handle_admin_login_page_application<Assets, Renderer>(
    request: AdminLoginPageApplicationRequest,
    assets: &Assets,
    renderer: &Renderer,
) -> PublicServingApplicationResponse
where
    Assets: PublicStaticAssetRead,
    Renderer: AdminLoginPageRenderer,
{
    // close a missing login template to the outer handler's ordinary error response
    let source = match assets.read_asset(StaticAssetRoot::Public, "admin-login.html") {
        Ok(source) => source,
        Err(_) => return public_internal_error(),
    };
    // retain release and optional invalid-credential markup in the template owner
    let body = match renderer.render_login_page(
        &source,
        AdminLoginPageRenderRequest {
            invalid_credentials: request.invalid_credentials,
        },
    ) {
        Ok(body) => body,
        Err(_) => return public_internal_error(),
    };
    let content_length = body.as_bytes().len();
    PublicServingApplicationResponse {
        // omit only the successful HEAD response after complete rendering
        body: PublicServingApplicationBody::Bytes(if request.omit_body {
            None
        } else {
            Some(body.0)
        }),
        header_profile: PublicServingHeaderProfile::Html {
            analytics_enabled: false,
        },
        headers: PublicServingApplicationHeaders {
            allow: None,
            cache_control: "no-store".to_owned(),
            content_length,
            content_type: "text/html; charset=utf-8".to_owned(),
            vary: Some("Cookie"),
        },
        status_code: 200,
        track_successful_forecast_response: false,
    }
}

/// read and optionally render one exact allowlisted static asset.
pub(crate) fn serve_static_asset<Assets, Renderer>(
    pathname: &str,
    plan: StaticAssetServingPlan<'_>,
    administrator: bool,
    assets: &Assets,
    renderer: &Renderer,
) -> PublicServingApplicationResponse
where
    Assets: PublicStaticAssetRead,
    Renderer: PublicStaticTemplateRenderer,
{
    let html_template = plan.template && plan.content_type == "text/html; charset=utf-8";
    let administrator = html_template && administrator;
    let analytics_enabled = html_template && !administrator && plan.analytics_allowed;
    // close any missing allowlisted file to the outer handler response
    let source = match assets.read_asset(plan.root, plan.relative_path) {
        Ok(source) => source,
        Err(_) => return public_internal_error(),
    };
    // retain release, digest, time, and preload rendering in the template owner
    let body = if plan.template {
        match renderer.render_template(
            &source,
            PublicStaticTemplateRenderRequest {
                administrator,
                analytics_enabled,
                pathname,
            },
        ) {
            Ok(body) => body,
            Err(_) => return public_internal_error(),
        }
    } else {
        source
    };
    let content_length = body.as_bytes().len();
    // select HTML policy only from the same admitted template metadata used above
    let header_profile = if html_template {
        PublicServingHeaderProfile::Html { analytics_enabled }
    } else {
        PublicServingHeaderProfile::Standard
    };
    PublicServingApplicationResponse {
        // omit only a HEAD body after the source file and template are resolved
        body: PublicServingApplicationBody::Bytes(if plan.omit_body { None } else { Some(body.0) }),
        header_profile,
        headers: PublicServingApplicationHeaders {
            allow: None,
            // keep the normal signed-in shell private on public routes too
            cache_control: if administrator {
                "private, no-store"
            } else {
                plan.cache_control
            }
            .to_owned(),
            content_length,
            content_type: plan.content_type.to_owned(),
            // vary only HTML documents by their representation cookie
            vary: if html_template { Some("Cookie") } else { None },
        },
        status_code: 200,
        track_successful_forecast_response: false,
    }
}

/// fetch one ordinary API target and apply its selected response policy.
fn serve_api_proxy<Api>(
    request: PublicServingApplicationRequest<'_>,
    plan: ApiProxyServingPlan,
    api: &Api,
) -> PublicServingApplicationResponse
where
    Api: PublicApiProxyClient,
{
    // close any transport or projection failure to the public proxy response
    let upstream = match api.fetch_api(PublicApiProxyClientRequest {
        accept: request.accept.unwrap_or(JSON_ACCEPT),
        maximum_response_bytes: plan.maximum_response_bytes,
        pathname: request.pathname,
        projection: plan.projection,
        search: request.search,
        timeout_ms: plan.timeout_ms,
        upstream_method: plan.upstream_method,
    }) {
        Ok(upstream) => upstream,
        Err(_) => return public_upstream_unavailable(),
    };

    // enforce the source cap on raw bytes, not on settings added by a successful projection
    let raw_body = !upstream.successful || plan.projection == PublicApiProjection::None;
    if plan.upstream_method == ApiUpstreamMethod::Get
        && raw_body
        && upstream.body.len() > plan.maximum_response_bytes
    {
        return public_upstream_unavailable();
    }
    // admit bounded upstream HEAD metadata without inventing a body
    let content_length = if plan.upstream_method == ApiUpstreamMethod::Head {
        let content_length = upstream.content_length.unwrap_or(0);
        if content_length > plan.maximum_response_bytes {
            return public_upstream_unavailable();
        }
        content_length
    } else {
        upstream.body.len()
    };
    // select content type ownership from the upstream result category
    let content_type_plan = if upstream.successful {
        plan.upstream_success_content_type
    } else {
        plan.upstream_non_success_content_type
    };
    let content_type = match content_type_plan {
        ApiContentTypePlan::Json => JSON_CONTENT_TYPE.to_owned(),
        ApiContentTypePlan::UpstreamOrJson => upstream
            .content_type
            .unwrap_or_else(|| JSON_CONTENT_TYPE.to_owned()),
    };
    PublicServingApplicationResponse {
        // preserve upstream bytes only for the caller's GET request
        body: PublicServingApplicationBody::Bytes(if plan.omit_body {
            None
        } else {
            Some(upstream.body)
        }),
        header_profile: PublicServingHeaderProfile::Standard,
        headers: PublicServingApplicationHeaders {
            allow: None,
            // never cache a failed or projected upstream response
            cache_control: if upstream.successful {
                plan.upstream_success_cache_control.to_owned()
            } else {
                plan.upstream_non_success_cache_control.to_owned()
            },
            content_length,
            content_type,
            // retain the source proxy's accept representation boundary
            vary: if plan.vary_accept {
                Some("Accept")
            } else {
                None
            },
        },
        status_code: upstream.status_code,
        track_successful_forecast_response: upstream.successful
            && plan.track_successful_forecast_response,
    }
}

/// build one exact shared text response without protocol I/O.
pub(crate) fn public_text_response(
    status_code: u16,
    body: &'static [u8],
    allow: Option<&'static str>,
) -> PublicServingApplicationResponse {
    PublicServingApplicationResponse {
        body: PublicServingApplicationBody::Text(body),
        header_profile: PublicServingHeaderProfile::Standard,
        headers: PublicServingApplicationHeaders {
            allow,
            cache_control: "no-store".to_owned(),
            content_length: body.len(),
            content_type: "text/plain; charset=utf-8".to_owned(),
            vary: None,
        },
        status_code,
        track_successful_forecast_response: false,
    }
}

/// build the outer public handler's static read failure response.
fn public_internal_error() -> PublicServingApplicationResponse {
    public_text_response(500, b"internal server error\n", None)
}

/// build the API proxy's closed upstream failure response.
fn public_upstream_unavailable() -> PublicServingApplicationResponse {
    public_text_response(502, b"upstream unavailable\n", None)
}
