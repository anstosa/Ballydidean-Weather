//! Native edge response writer retaining forecast evidence until the socket terminal.

use std::fmt::{self, Display, Formatter};

use hyper::{Response, http};
use weather_edge::{
    EdgeForecastTerminalObserver, EdgeWireAdminCookie, EdgeWireBody, EdgeWireHeaders,
    EdgeWireReply, EdgeWireWriter, ForecastResponseTerminal, PublicServingHeaderProfile,
    emit_edge_wire_response,
};

use crate::{
    NativeHttpBody, NativeHttpConnectionScope, NativeHttpTerminalError, NativeTrackedHttpBody,
    native_edge_http_head,
};

/// Refuse an incomplete or mismatched edge response before Hyper receives it.
#[derive(Debug)]
pub enum NativeEdgeHttpResponseError {
    Header(http::Error),
    Terminal(NativeHttpTerminalError),
    InvalidResponse,
}

impl Display for NativeEdgeHttpResponseError {
    /// Retain a categorical transport failure with its underlying error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Header(error) => write!(formatter, "edge HTTP header: {error}"),
            Self::Terminal(error) => write!(formatter, "edge HTTP terminal: {error}"),
            Self::InvalidResponse => formatter.write_str("invalid edge HTTP response"),
        }
    }
}

impl std::error::Error for NativeEdgeHttpResponseError {}

/// Move one real application reply to Hyper and bind its evidence to this connection.
pub fn native_edge_http_response<Prepared, Observe>(
    reply: EdgeWireReply<Prepared>,
    scope: NativeHttpConnectionScope,
    observe: Observe,
) -> Result<Response<NativeTrackedHttpBody>, NativeEdgeHttpResponseError>
where
    Prepared: 'static,
    Observe: FnOnce(EdgeForecastTerminalObserver<Prepared>, ForecastResponseTerminal) + 'static,
{
    let mut writer = NativeEdgeHttpWriter {
        scope,
        head: None,
        observer: None,
        observe: Some(observe),
        response: None,
    };
    emit_edge_wire_response(reply, &mut writer)?;
    writer
        .response
        .take()
        .ok_or(NativeEdgeHttpResponseError::InvalidResponse)
}

struct NativeEdgeHttpWriter<Prepared, Observe>
where
    Observe: FnOnce(EdgeForecastTerminalObserver<Prepared>, ForecastResponseTerminal),
{
    scope: NativeHttpConnectionScope,
    head: Option<(http::response::Builder, usize, u16)>,
    observer: Option<EdgeForecastTerminalObserver<Prepared>>,
    observe: Option<Observe>,
    response: Option<Response<NativeTrackedHttpBody>>,
}

impl<Prepared, Observe> EdgeWireWriter<Prepared> for NativeEdgeHttpWriter<Prepared, Observe>
where
    Prepared: 'static,
    Observe: FnOnce(EdgeForecastTerminalObserver<Prepared>, ForecastResponseTerminal) + 'static,
{
    type Error = NativeEdgeHttpResponseError;

    /// Retain the exact application head before moving any body bytes.
    fn write_head(
        &mut self,
        status_code: u16,
        profile: PublicServingHeaderProfile,
        headers: &EdgeWireHeaders,
        admin_cookie: Option<&EdgeWireAdminCookie>,
    ) -> Result<(), Self::Error> {
        self.head = Some((
            native_edge_http_head(status_code, profile, headers, admin_cookie),
            headers.content_length,
            status_code,
        ));
        Ok(())
    }

    /// Keep the selected observer until its finite forecast representation is known.
    fn install_forecast_terminal(
        &mut self,
        observer: EdgeForecastTerminalObserver<Prepared>,
    ) -> Result<(), Self::Error> {
        self.observer = Some(observer);
        Ok(())
    }

    /// Bind exact-length forecast bytes to the real socket accounting owner.
    fn send_body(&mut self, body: EdgeWireBody) -> Result<(), Self::Error> {
        let (head, length, status) = self.head.take().ok_or(Self::Error::InvalidResponse)?;
        let response = head.body(()).map_err(Self::Error::Header)?;
        // only a nonempty exact successful forecast representation can own capture
        if self.observer.is_some()
            && (!(200..300).contains(&status)
                || matches!(status, 204 | 205)
                || length == 0
                || length != body.as_bytes().len())
        {
            return Err(Self::Error::InvalidResponse);
        }
        let body = NativeHttpBody::from_edge(body);
        let framed = match self.observer.take() {
            Some(observer) => {
                let observe = self.observe.take().ok_or(Self::Error::InvalidResponse)?;
                self.scope
                    .tracked_body(body, move |terminal| observe(observer, terminal))
                    .map_err(Self::Error::Terminal)?
            }
            None => self.scope.untracked_body(body),
        };
        self.response = Some(response.map(|()| framed));
        Ok(())
    }
}

impl<Prepared, Observe> Drop for NativeEdgeHttpWriter<Prepared, Observe>
where
    Observe: FnOnce(EdgeForecastTerminalObserver<Prepared>, ForecastResponseTerminal),
{
    /// Close an observer still owned before the response could reach the socket.
    fn drop(&mut self) {
        // a failed head or invalid body never earns a finished event
        if let (Some(observer), Some(observe)) = (self.observer.take(), self.observe.take()) {
            observe(observer, ForecastResponseTerminal::Closed);
        }
    }
}
