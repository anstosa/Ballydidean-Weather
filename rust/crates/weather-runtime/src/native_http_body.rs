//! Finite HTTP bodies and API response metadata on the selected Hyper transport.

use std::{
    convert::Infallible,
    future::poll_fn,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use hyper::{
    Response,
    body::{Body, Bytes, Frame, Incoming, SizeHint},
    header::{ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
};
use weather_api::{AdjustmentMaintenanceWireResponse, WeatherServiceWireResponse};
use weather_edge::EdgeWireBody;

/// One finite frame retaining the application's original owned representation.
#[derive(Debug, Default)]
pub struct NativeHttpBody(Option<Bytes>);

impl NativeHttpBody {
    /// Take already allocated bytes without copying their payload.
    pub fn new(bytes: Bytes) -> Self {
        Self((!bytes.is_empty()).then_some(bytes))
    }

    /// Move owned or shared edge storage into a single immutable frame.
    pub fn from_edge(body: EdgeWireBody) -> Self {
        // preserve the original backing allocation regardless of its storage variant
        match body {
            EdgeWireBody::Empty => Self::default(),
            EdgeWireBody::Static(bytes) => Self::new(Bytes::from_static(bytes)),
            EdgeWireBody::Owned(bytes) => Self::new(Bytes::from(bytes)),
            body => Self::new(Bytes::from_owner(SharedEdgeBody(body))),
        }
    }
}

struct SharedEdgeBody(EdgeWireBody);

impl AsRef<[u8]> for SharedEdgeBody {
    /// Expose shared response bytes without cloning their payload.
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl Body for NativeHttpBody {
    type Data = Bytes;
    type Error = Infallible;

    /// Submit the remaining frame once.
    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(self.get_mut().0.take().map(|bytes| Ok(Frame::data(bytes))))
    }

    /// Indicate when no more representation bytes remain.
    fn is_end_stream(&self) -> bool {
        self.0.is_none()
    }

    /// Enable exact HTTP framing without a chunked representation.
    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.0.as_ref().map_or(0, |bytes| bytes.len() as u64))
    }
}

/// Bind the original public API metadata to Hyper before releasing the encoder scratch.
pub fn native_api_http_response(
    wire: WeatherServiceWireResponse<'_>,
) -> Result<Response<NativeHttpBody>, hyper::http::Error> {
    let mut response = Response::builder()
        .status(wire.status)
        .header(CONTENT_TYPE, wire.headers.content_type);
    // retain the application-selected cache metadata
    if let Some(cache_control) = wire.headers.cache_control {
        response = response.header(CACHE_CONTROL, cache_control);
    }
    // retain the source method advertisement on method refusals
    if let Some(allow) = wire.headers.allow {
        response = response.header(ALLOW, allow);
    }
    response.body(NativeHttpBody::new(Bytes::copy_from_slice(wire.body)))
}

/// Bind the original private maintenance status, content type and representation length.
pub fn native_maintenance_http_response(
    wire: AdjustmentMaintenanceWireResponse<'_>,
) -> Result<Response<NativeHttpBody>, hyper::http::Error> {
    Response::builder()
        .status(wire.status_code)
        .header(CONTENT_TYPE, wire.headers.content_type)
        .header(CONTENT_LENGTH, wire.headers.content_length)
        .body(NativeHttpBody::new(Bytes::copy_from_slice(
            wire.body.as_bytes(),
        )))
}

/// Ordinary bounded body collection outcomes for the owning application to map.
#[derive(Debug)]
pub enum NativeHttpBodyReadError {
    TooLarge,
    Deadline,
    Transport(hyper::Error),
}

/// Read one request body under the caller's existing route size and time limits.
pub async fn read_native_http_body(
    mut body: Incoming,
    maximum: usize,
    deadline: Duration,
) -> Result<Vec<u8>, NativeHttpBodyReadError> {
    // avoid allocating for declared representations above the admitted capacity
    if body.size_hint().lower() > maximum as u64 {
        return Err(NativeHttpBodyReadError::TooLarge);
    }
    tokio::time::timeout(deadline, async {
        let mut bytes = Vec::new();
        // consume only the protocol-decoded body frames
        while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
            let frame = frame.map_err(NativeHttpBodyReadError::Transport)?;
            // data frames contain the application body rather than transport metadata
            if let Ok(data) = frame.into_data() {
                // enforce the actual cumulative capacity before extending storage
                if data.len() > maximum.saturating_sub(bytes.len()) {
                    return Err(NativeHttpBodyReadError::TooLarge);
                }
                bytes.extend_from_slice(&data);
            }
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| NativeHttpBodyReadError::Deadline)?
}
