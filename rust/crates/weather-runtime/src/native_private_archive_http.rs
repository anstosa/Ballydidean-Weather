//! Actual private archive request collection and reply materialization on Hyper.

use std::time::Duration;

use hyper::{
    Request, Response,
    body::Incoming,
    header::CONTENT_TYPE,
    http::{self, request::Parts},
};
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, EdgeWireBody, PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES,
    PrivateArchiveErrorBody, PrivateArchiveResponse, PrivateArchiveResponseBody,
    PrivateArchiveStatus, PrivateArchiveStorage, PrivateArchiveTransportRequest,
    PrivateArchiveWireBody, admit_private_archive_transport,
    materialize_private_archive_wire_response, serve_private_archive_wire_request,
};

use crate::{NativeHttpBody, native_edge_http_head, read_native_http_body};

/// Read only an admitted private archive body before acquiring the synchronous store owner.
pub struct NativePrivateArchiveHttpRequest {
    parts: Parts,
    body: Result<Vec<u8>, PrivateArchiveResponse>,
}

impl NativePrivateArchiveHttpRequest {
    /// Apply the original listener's metadata admission before collecting request bytes.
    pub async fn read(request: Request<Incoming>, body_deadline: Duration) -> Self {
        let (parts, body) = request.into_parts();
        let body = match admit_private_archive_transport(transport(&parts)) {
            // leave rejected requests unread
            Err((status, error)) => Err(refusal(status, error)),
            Ok(_) => read_native_http_body(body, PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES, body_deadline)
                .await
                .map_err(|_| {
                    refusal(
                        PrivateArchiveStatus::Conflict,
                        PrivateArchiveErrorBody::RevisionArchiveRejected,
                    )
                }),
        };
        Self { parts, body }
    }

    /// Dispatch through the actual archive stores and move their finite response into Hyper.
    pub fn serve<P: ArchiveWirePrimitives, Storage: PrivateArchiveStorage>(
        self,
        storage: &mut Storage,
        codec: &ArchiveWireCodec<P>,
        encoder: &mut ArchiveWireCodec<P>,
    ) -> Result<Response<NativeHttpBody>, http::Error> {
        let mut wire = match self.body {
            Ok(bytes) => serve_private_archive_wire_request(
                transport(&self.parts),
                &mut PrivateArchiveWireBody::new(&bytes, codec),
                storage,
                encoder,
            ),
            Err(response) => materialize_private_archive_wire_response(response, encoder),
        };
        // suppress head representations without changing their original length
        if self.parts.method == hyper::Method::HEAD {
            wire.body = EdgeWireBody::Empty;
        }

        // use the shared source response-policy owner for both listeners
        native_edge_http_head(
            wire.status_code,
            wire.header_profile,
            &wire.headers,
            wire.admin_cookie.as_ref(),
        )
        .body(NativeHttpBody::from_edge(wire.body))
    }
}

/// Preserve the raw request target and media type for the original listener admission.
fn transport(parts: &Parts) -> PrivateArchiveTransportRequest<'_> {
    // the private listener recognizes origin-form targets only
    let target = if parts.uri.scheme().is_some() || parts.uri.authority().is_some() {
        ""
    } else {
        parts
            .uri
            .path_and_query()
            .map_or("", |target| target.as_str())
    };
    PrivateArchiveTransportRequest {
        method: parts.method.as_str(),
        target,
        content_type: parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
    }
}

/// Preserve the listener's selected bounded error without invoking archive storage.
fn refusal(status: PrivateArchiveStatus, error: PrivateArchiveErrorBody) -> PrivateArchiveResponse {
    PrivateArchiveResponse {
        status,
        body: PrivateArchiveResponseBody::Error(error),
    }
}
