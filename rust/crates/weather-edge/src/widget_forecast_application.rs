use crate::{WidgetForecastUpstream, WidgetForecastVersion, supported_widget_forecast};

const MAXIMUM_API_BYTES: usize = 1024 * 1024;
const WIDGET_FORECAST_MAX_BYTES: usize = 128 * 1024;
const WIDGET_FORECAST_TIMEOUT_MS: u64 = 5_000;

/// one approved canonical receipt instant resolved at projection time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedWidgetForecastReceivedAt(String);

impl AdmittedWidgetForecastReceivedAt {
    /// admit one canonical receipt instant from the owning time adapter.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the admitted canonical receipt instant.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// one complete compact snapshot admitted by the existing versioned projector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedWidgetForecastSnapshotJson {
    content: Vec<u8>,
    version: WidgetForecastVersion,
}

impl AdmittedWidgetForecastSnapshotJson {
    /// admit one encoded snapshot without the edge response newline.
    pub fn admitted(version: WidgetForecastVersion, content: impl Into<Vec<u8>>) -> Self {
        Self {
            content: content.into(),
            version,
        }
    }

    /// borrow the admitted encoded snapshot.
    pub fn as_bytes(&self) -> &[u8] {
        &self.content
    }

    /// return the projector version bound to these bytes.
    pub const fn version(&self) -> WidgetForecastVersion {
        self.version
    }
}

/// one parsed widget request method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetForecastApplicationMethod {
    Get,
    Head,
    Unsupported,
}

/// one transport-independent request after URL parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetForecastApplicationRequest<'a> {
    pub has_query: bool,
    pub method: WidgetForecastApplicationMethod,
    pub pathname: &'a str,
}

/// bounded typed fetch selected for one supported projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetForecastClientRequest {
    pub maximum_response_bytes: usize,
    pub timeout_ms: u64,
    pub upstream: WidgetForecastUpstream,
    pub version: WidgetForecastVersion,
}

/// upstream loading and settings filtering retained outside route policy.
pub trait WidgetForecastClient {
    type Error;
    type Forecast;

    /// fetch and filter one typed forecast for the selected projection.
    fn fetch_forecast(
        &self,
        request: WidgetForecastClientRequest,
    ) -> Result<Self::Forecast, Self::Error>;
}

/// canonical receipt-time resolution retained outside this route policy.
pub trait WidgetForecastReceivedAtSource {
    type Error;

    /// resolve one approved receipt instant after the forecast is loaded.
    fn approved_received_at(&self) -> Result<AdmittedWidgetForecastReceivedAt, Self::Error>;
}

/// versioned projection and encoding retained in the existing projection owner.
pub trait WidgetForecastSnapshotProjector<Forecast> {
    type Error;

    /// project one loaded forecast at the approved receipt instant.
    fn project_snapshot(
        &self,
        forecast: &Forecast,
        received_at: &AdmittedWidgetForecastReceivedAt,
        version: WidgetForecastVersion,
    ) -> Result<AdmittedWidgetForecastSnapshotJson, Self::Error>;
}

/// one response body retained for the owning protocol adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WidgetForecastApplicationBody {
    Text(&'static [u8]),
    Json(Option<Vec<u8>>),
}

/// exact ordinary response headers for the public widget route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetForecastApplicationHeaders {
    pub allow: Option<&'static str>,
    pub cache_control: &'static str,
    pub content_length: usize,
    pub content_type: &'static str,
}

/// one complete transport-independent widget response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetForecastApplicationResponse {
    pub body: WidgetForecastApplicationBody,
    pub headers: WidgetForecastApplicationHeaders,
    pub status_code: u16,
}

/// serve one exact public widget route over typed fetch and projection ports.
pub fn handle_widget_forecast_application<Client, Clock, Projector>(
    request: WidgetForecastApplicationRequest<'_>,
    client: &Client,
    clock: &Clock,
    projector: &Projector,
) -> WidgetForecastApplicationResponse
where
    Client: WidgetForecastClient,
    Clock: WidgetForecastReceivedAtSource,
    Projector: WidgetForecastSnapshotProjector<Client::Forecast>,
{
    // select only one exact reviewed versioned widget route
    let Some((version, upstream)) = supported_widget_forecast(request.pathname) else {
        return widget_text_response(404, b"not found\n", None);
    };

    // reject caller-controlled projection inputs before method handling
    if request.has_query {
        return widget_text_response(400, b"bad request\n", None);
    }

    // retain the source GET and HEAD response distinction
    let omit_body = match request.method {
        WidgetForecastApplicationMethod::Get => false,
        WidgetForecastApplicationMethod::Head => true,
        WidgetForecastApplicationMethod::Unsupported => {
            return widget_text_response(405, b"method not allowed\n", Some("GET, HEAD"));
        }
    };
    let client_request = WidgetForecastClientRequest {
        maximum_response_bytes: MAXIMUM_API_BYTES,
        timeout_ms: WIDGET_FORECAST_TIMEOUT_MS,
        upstream,
        version,
    };
    // close every upstream or settings failure to one public response
    let forecast = match client.fetch_forecast(client_request) {
        Ok(forecast) => forecast,
        Err(_) => return widget_upstream_unavailable(),
    };
    // resolve the canonical receipt only after the upstream load succeeds
    let received_at = match clock.approved_received_at() {
        Ok(received_at) => received_at,
        Err(_) => return widget_upstream_unavailable(),
    };
    // preserve the existing projector as the sole snapshot semantics owner
    let snapshot = match projector.project_snapshot(&forecast, &received_at, version) {
        Ok(snapshot) => snapshot,
        Err(_) => return widget_upstream_unavailable(),
    };

    // reject a projector result bound to any other public contract
    if snapshot.version() != version {
        return widget_upstream_unavailable();
    }
    // include the required final newline without accepting length overflow
    let Some(content_length) = snapshot.as_bytes().len().checked_add(1) else {
        return widget_upstream_unavailable();
    };

    // enforce the source response ceiling including its final newline
    if content_length > WIDGET_FORECAST_MAX_BYTES {
        return widget_upstream_unavailable();
    }
    let mut content = snapshot.content;
    content.push(b'\n');
    WidgetForecastApplicationResponse {
        // omit only the successful HEAD response body after full projection
        body: WidgetForecastApplicationBody::Json(if omit_body { None } else { Some(content) }),
        headers: WidgetForecastApplicationHeaders {
            allow: None,
            cache_control: "no-store",
            content_length,
            content_type: "application/json; charset=utf-8",
        },
        status_code: 200,
    }
}

/// build one exact bounded text response without protocol I/O.
fn widget_text_response(
    status_code: u16,
    body: &'static [u8],
    allow: Option<&'static str>,
) -> WidgetForecastApplicationResponse {
    WidgetForecastApplicationResponse {
        body: WidgetForecastApplicationBody::Text(body),
        headers: WidgetForecastApplicationHeaders {
            allow,
            cache_control: "no-store",
            content_length: body.len(),
            content_type: "text/plain; charset=utf-8",
        },
        status_code,
    }
}

/// build the source handler's closed upstream failure response.
fn widget_upstream_unavailable() -> WidgetForecastApplicationResponse {
    widget_text_response(502, b"upstream unavailable\n", None)
}
