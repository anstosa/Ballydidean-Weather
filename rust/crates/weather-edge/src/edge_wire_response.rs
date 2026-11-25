//! Move typed edge results into the application response consumed by the HTTP adapter.

use std::borrow::Cow;
use std::sync::Arc;

use crate::edge_wire_admin_response::{EdgeWireAdminCookie, materialize_admin_response};
use crate::edge_wire_json_responses::{
    EdgeWireJsonEncoder, materialize_adjustment_settings, materialize_property_sensor_layout,
    materialize_viewer_context,
};
use crate::{
    EdgeForecastCaptureAdmission, EdgeForecastTerminalObserver, ForecastAdjustmentScorecardBody,
    ForecastEvidenceWindow, ForecastRecordedProductBody, ForecastResponseEvidenceStore,
    PublicServingApplicationBody, PublicServingHeaderProfile, WeatherEdgeDispatchBoundary,
    WeatherEdgeDispatchOutcome, WeatherEdgeDispatchPortSet, WeatherEdgeDispatchRequest,
    WidgetForecastApplicationBody, XweatherMapProxyBody, XweatherTileBody, XweatherTileCacheStatus,
    dispatch_edge_reply,
};

const TEXT_CONTENT_TYPE: &str = "text/plain; charset=utf-8";
const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";

/// Retain the selected representation without cloning bytes or allocating header lists.
#[derive(Debug, PartialEq, Eq)]
pub enum EdgeWireBody {
    Empty,
    Static(&'static [u8]),
    Owned(Vec<u8>),
    Shared(Arc<[u8]>),
    Tile(XweatherTileBody),
}

impl EdgeWireBody {
    /// Borrow the emitted bytes while retaining their original storage owner.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Empty => &[],
            Self::Static(bytes) => bytes,
            Self::Owned(bytes) => bytes,
            Self::Shared(bytes) => bytes,
            Self::Tile(bytes) => bytes.as_bytes(),
        }
    }

    /// Report whether any bytes would be sent with this response.
    pub fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }

    /// Move a representation's already serialized body or retain its omitted state.
    fn owned(body: Option<Vec<u8>>) -> Self {
        body.map_or(Self::Empty, Self::Owned)
    }
}

/// Exact ordinary headers; connection and retained policy headers belong to the wire writer.
#[derive(Debug, PartialEq, Eq)]
pub struct EdgeWireHeaders {
    pub allow: Option<&'static str>,
    pub cache_control: Cow<'static, str>,
    pub content_length: usize,
    pub content_type: Option<Cow<'static, str>>,
    pub location: Option<Cow<'static, str>>,
    pub retry_after: Option<Cow<'static, str>>,
    pub tile_age_seconds: Option<u64>,
    pub tile_cache: Option<XweatherTileCacheStatus>,
    pub vary: Option<&'static str>,
}

/// One complete transport-independent response and its existing header policy selection.
#[derive(Debug, PartialEq, Eq)]
pub struct EdgeWireResponse {
    pub admin_cookie: Option<EdgeWireAdminCookie>,
    pub body: EdgeWireBody,
    pub header_profile: PublicServingHeaderProfile,
    pub headers: EdgeWireHeaders,
    pub status_code: u16,
}

impl EdgeWireResponse {
    /// Initialize ordinary representation metadata without allocating static strings.
    fn ordinary(
        status_code: u16,
        body: EdgeWireBody,
        cache_control: impl Into<Cow<'static, str>>,
        content_length: usize,
        content_type: Option<Cow<'static, str>>,
        allow: Option<&'static str>,
    ) -> Self {
        Self {
            admin_cookie: None,
            body,
            header_profile: PublicServingHeaderProfile::Standard,
            headers: EdgeWireHeaders {
                allow,
                cache_control: cache_control.into(),
                content_length,
                content_type,
                location: None,
                retry_after: None,
                tile_age_seconds: None,
                tile_cache: None,
                vary: None,
            },
            status_code,
        }
    }

    /// Select fixed error bytes and preserve their would-be body length on head.
    pub(crate) fn text(
        status_code: u16,
        body: &'static [u8],
        cache_control: &'static str,
        allow: Option<&'static str>,
    ) -> Self {
        Self::ordinary(
            status_code,
            EdgeWireBody::Static(body),
            cache_control,
            body.len(),
            Some(Cow::Borrowed(TEXT_CONTENT_TYPE)),
            allow,
        )
    }

    /// Keep one encoded representation and use its full length even when the body is omitted.
    pub(crate) fn json(
        status_code: u16,
        body: Vec<u8>,
        omit_body: bool,
        cache_control: &'static str,
    ) -> Self {
        let length = body.len();
        Self::ordinary(
            status_code,
            EdgeWireBody::owned((!omit_body).then_some(body)),
            cache_control,
            length,
            Some(Cow::Borrowed(JSON_CONTENT_TYPE)),
            None,
        )
    }

    /// Emit a finite, canonical JSON representation with no runtime allocation.
    pub(crate) fn fixed_json(
        status_code: u16,
        body: &'static [u8],
        cache_control: &'static str,
    ) -> Self {
        Self::ordinary(
            status_code,
            EdgeWireBody::Static(body),
            cache_control,
            body.len(),
            Some(Cow::Borrowed(JSON_CONTENT_TYPE)),
            None,
        )
    }

    /// Retain the exact Location without creating a body or content type.
    pub(crate) fn redirect(
        status_code: u16,
        location: impl Into<Cow<'static, str>>,
        cache_control: &'static str,
    ) -> Self {
        let mut response = Self::ordinary(
            status_code,
            EdgeWireBody::Empty,
            cache_control,
            0,
            None,
            None,
        );
        response.headers.location = Some(location.into());
        response
    }
}

/// Retain each response's assigned terminal observer until the owning writer receives it.
pub struct EdgeWireReply<Prepared> {
    response: EdgeWireResponse,
    observer: Option<EdgeForecastTerminalObserver<Prepared>>,
}

impl<Prepared> EdgeWireReply<Prepared> {
    /// Inspect the selected representation without separating it from its observer.
    pub const fn response(&self) -> &EdgeWireResponse {
        &self.response
    }
}

impl EdgeWireReply<()> {
    /// Route a private or other non-forecast response through the same writer.
    pub const fn without_forecast(response: EdgeWireResponse) -> Self {
        Self {
            response,
            observer: None,
        }
    }
}

/// Materialize a terminal administrator outcome using the same cookie and response owner as dispatch.
pub fn edge_terminal_admin_reply(
    outcome: crate::WeatherAdminRouteOutcome,
    head: bool,
) -> EdgeWireReply<()> {
    let mut response = materialize_admin_response(outcome);
    // preserve the would-be content length while suppressing every HEAD body
    if head {
        response.body = EdgeWireBody::Empty;
    }
    EdgeWireReply::without_forecast(response)
}

/// Materialize an ordinary ingress refusal without creating a forecast observer.
pub fn edge_terminal_text_reply(
    status_code: u16,
    body: &'static [u8],
    allow: Option<&'static str>,
    head: bool,
) -> EdgeWireReply<()> {
    let mut response = EdgeWireResponse::text(status_code, body, "no-store", allow);
    // preserve the same ordinary HEAD response representation
    if head {
        response.body = EdgeWireBody::Empty;
    }
    EdgeWireReply::without_forecast(response)
}

/// Retain request admission boundaries without substituting an ordinary wire response.
pub type EdgeWireDispatchOutcome<Reply> = Result<Reply, WeatherEdgeDispatchBoundary>;

/// Dispatch one admitted request and retain its independent real-terminal observer.
pub fn handle_edge_wire_request<Ports, Store, Encoder>(
    request: WeatherEdgeDispatchRequest<'_>,
    ports: Ports,
    capture: EdgeForecastCaptureAdmission,
    normalize_window: impl FnOnce(&str) -> Option<ForecastEvidenceWindow>,
    store: &mut Store,
    encoder: &mut Encoder,
) -> EdgeWireDispatchOutcome<EdgeWireReply<Store::Prepared>>
where
    Ports: WeatherEdgeDispatchPortSet,
    Store: ForecastResponseEvidenceStore,
    Encoder: EdgeWireJsonEncoder,
{
    let head = request.method == "HEAD";
    let (outcome, observer) = dispatch_edge_reply(request, ports, capture, normalize_window, store);
    let wire = materialize_edge_wire_response(outcome, encoder).unwrap_or_else(
        // contain ordinary encoding failures before the adapter emits headers
        |_| {
            Ok(EdgeWireResponse::text(
                500,
                b"internal server error\n",
                "no-store",
                None,
            ))
        },
    );
    wire.map(
        // keep the forecast observer bound to only the reply selected for this request
        |mut response| {
            // the source transport suppresses every head body, including a text error
            if head {
                response.body = EdgeWireBody::Empty;
            }
            EdgeWireReply {
                response,
                observer: observer.is_pending().then_some(observer),
            }
        },
    )
}

/// Transfer each already-admitted application result into the proper wire owner.
fn materialize_edge_wire_response<Encoder: EdgeWireJsonEncoder>(
    outcome: WeatherEdgeDispatchOutcome,
    encoder: &mut Encoder,
) -> Result<EdgeWireDispatchOutcome<EdgeWireResponse>, Encoder::Error> {
    let response = match outcome {
        WeatherEdgeDispatchOutcome::Public(response) => {
            // move the exact outgoing api or asset buffer without re-encoding it
            let body = match response.body {
                PublicServingApplicationBody::Text(bytes) => EdgeWireBody::Static(bytes),
                PublicServingApplicationBody::Bytes(bytes) => EdgeWireBody::owned(bytes),
            };
            let mut wire = EdgeWireResponse::ordinary(
                response.status_code,
                body,
                response.headers.cache_control,
                response.headers.content_length,
                Some(Cow::Owned(response.headers.content_type)),
                response.headers.allow,
            );
            wire.header_profile = response.header_profile;
            wire.headers.vary = response.headers.vary;
            wire
        }
        WeatherEdgeDispatchOutcome::Widget(response) => {
            // preserve the selected widget encoding and already computed head length
            let body = match response.body {
                WidgetForecastApplicationBody::Text(bytes) => EdgeWireBody::Static(bytes),
                WidgetForecastApplicationBody::Json(bytes) => EdgeWireBody::owned(bytes),
            };
            EdgeWireResponse::ordinary(
                response.status_code,
                body,
                response.headers.cache_control,
                response.headers.content_length,
                Some(Cow::Borrowed(response.headers.content_type)),
                response.headers.allow,
            )
        }
        WeatherEdgeDispatchOutcome::Recorded(response) => {
            // preserve shared history bytes and the ordinary retry metadata
            let body = match response.body {
                ForecastRecordedProductBody::Text(bytes) => EdgeWireBody::Static(bytes),
                ForecastRecordedProductBody::Json(bytes) => {
                    bytes.map_or(EdgeWireBody::Empty, EdgeWireBody::Shared)
                }
            };
            let mut wire = EdgeWireResponse::ordinary(
                response.status_code,
                body,
                response.headers.cache_control,
                response.headers.content_length,
                Some(Cow::Borrowed(response.headers.content_type)),
                response.headers.allow,
            );
            wire.headers.retry_after = response.headers.retry_after.map(Cow::Borrowed);
            wire
        }
        WeatherEdgeDispatchOutcome::Map(response) => {
            // transfer the shared tile allocation without copying its png bytes
            let body = match response.body {
                XweatherMapProxyBody::Text(bytes) => EdgeWireBody::Static(bytes),
                XweatherMapProxyBody::Png(bytes) => {
                    bytes.map_or(EdgeWireBody::Empty, EdgeWireBody::Tile)
                }
            };
            let mut wire = EdgeWireResponse::ordinary(
                response.status_code,
                body,
                response.headers.cache_control,
                response.headers.content_length,
                Some(Cow::Borrowed(response.headers.content_type)),
                response.headers.allow,
            );
            wire.headers.retry_after = response.headers.retry_after.map(Cow::Owned);
            wire.headers.tile_age_seconds = response.headers.tile_age_seconds;
            wire.headers.tile_cache = response.headers.tile_cache;
            wire
        }
        WeatherEdgeDispatchOutcome::AdjustmentScorecard(response) => {
            // preserve the pre-encoded aggregate and any ordinary retry delay
            let body = match response.body {
                ForecastAdjustmentScorecardBody::Text(bytes) => EdgeWireBody::Static(bytes),
                ForecastAdjustmentScorecardBody::Json(bytes) => EdgeWireBody::owned(bytes),
            };
            let mut wire = EdgeWireResponse::ordinary(
                response.status_code,
                body,
                response.headers.cache_control,
                response.headers.content_length,
                Some(Cow::Borrowed(response.headers.content_type)),
                response.headers.allow,
            );
            wire.headers.retry_after = response.headers.retry_after.map(Cow::Borrowed);
            wire
        }
        WeatherEdgeDispatchOutcome::AdjustmentSettings(outcome) => {
            materialize_adjustment_settings(outcome, encoder)?
        }
        WeatherEdgeDispatchOutcome::PropertySensorLayout(outcome) => {
            materialize_property_sensor_layout(outcome, encoder)?
        }
        WeatherEdgeDispatchOutcome::ViewerContext(outcome) => {
            materialize_viewer_context(outcome, encoder)?
        }
        WeatherEdgeDispatchOutcome::RemoteAgentsPreview(response) => {
            // move the source redirect's location without allocating it again
            let mut wire = EdgeWireResponse::redirect(
                response.status_code,
                response.location,
                response.cache_control,
            );
            wire.headers.content_length = response.content_length;
            wire
        }
        WeatherEdgeDispatchOutcome::Admin(outcome) => materialize_admin_response(outcome),
        WeatherEdgeDispatchOutcome::Boundary(boundary) => {
            return Ok(Err(boundary));
        }
    };
    Ok(Ok(response))
}

/// The approved HTTP adapter supplies wire formatting and all retained header policies.
pub trait EdgeWireWriter<Prepared> {
    type Error;

    /// Emit the selected ordinary and policy headers before starting the body.
    fn write_head(
        &mut self,
        status_code: u16,
        profile: PublicServingHeaderProfile,
        headers: &EdgeWireHeaders,
        admin_cookie: Option<&EdgeWireAdminCookie>,
    ) -> Result<(), Self::Error>;

    /// Register the native finish/close callback without observing an event yet.
    fn install_forecast_terminal(
        &mut self,
        observer: EdgeForecastTerminalObserver<Prepared>,
    ) -> Result<(), Self::Error>;

    /// Move the body to the transport; this is not a forecast finish notification.
    fn send_body(&mut self, body: EdgeWireBody) -> Result<(), Self::Error>;
}

/// Hand one materialized response to its writer without synthesizing completion events.
pub fn emit_edge_wire_response<Prepared, Writer: EdgeWireWriter<Prepared>>(
    reply: EdgeWireReply<Prepared>,
    writer: &mut Writer,
) -> Result<(), Writer::Error> {
    let response = reply.response;
    writer.write_head(
        response.status_code,
        response.header_profile,
        &response.headers,
        response.admin_cookie.as_ref(),
    )?;
    // bind actual finish/close after headers and before the body can finish synchronously
    if let Some(observer) = reply.observer {
        writer.install_forecast_terminal(observer)?;
    }
    writer.send_body(response.body)
}
