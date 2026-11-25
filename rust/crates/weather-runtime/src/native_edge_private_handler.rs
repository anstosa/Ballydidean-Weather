//! Concrete private archive Hyper ownership over one retained blocking storage lane.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use hyper::{Request, Response, body::Incoming, header::CONTENT_TYPE, http};
use tokio::sync::Semaphore;
use weather_edge::{
    ArchiveWireCodec, EdgeWireBody, EdgeWireResponse, PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES,
    PrivateArchiveErrorBody, PrivateArchiveResponse, PrivateArchiveResponseBody,
    PrivateArchiveStatus, PrivateArchiveStorage, PrivateArchiveTransportRequest,
    PrivateArchiveWireBody, admit_private_archive_transport,
    materialize_private_archive_wire_response, serve_private_archive_wire_request,
};

use crate::{NativeHttpBody, NativeWirePrimitives, native_edge_http_head, read_native_http_body};

/// Owned metadata that can cross into the bounded blocking storage worker.
struct NativePrivateArchiveTarget {
    content_type: Option<String>,
    method: String,
    target: String,
}

impl NativePrivateArchiveTarget {
    /// Borrow the exact transport fields expected by the archive service.
    fn request(&self) -> PrivateArchiveTransportRequest<'_> {
        PrivateArchiveTransportRequest {
            method: &self.method,
            target: &self.target,
            content_type: self.content_type.as_deref(),
        }
    }
}

/// Cloneable private listener handler retaining one genuine source-ordered storage owner.
pub struct NativeEdgePrivateHandler<Storage> {
    body_deadline: Duration,
    lane: Arc<Semaphore>,
    storage: Arc<Mutex<Storage>>,
}

impl<Storage> Clone for NativeEdgePrivateHandler<Storage> {
    /// Share the one real storage owner and its retained source-order admission lane.
    fn clone(&self) -> Self {
        Self {
            body_deadline: self.body_deadline,
            lane: Arc::clone(&self.lane),
            storage: Arc::clone(&self.storage),
        }
    }
}

impl<Storage> NativeEdgePrivateHandler<Storage> {
    /// Bind a genuine archive storage port without reading or substituting process state.
    pub fn new(storage: Storage, body_deadline: Duration) -> Self {
        Self {
            body_deadline,
            lane: Arc::new(Semaphore::new(1)),
            storage: Arc::new(Mutex::new(storage)),
        }
    }

    /// Share the retained storage identity for process composition and lifecycle evidence.
    pub fn shared_storage(&self) -> Arc<Mutex<Storage>> {
        Arc::clone(&self.storage)
    }
}

impl<Storage> NativeEdgePrivateHandler<Storage>
where
    Storage: PrivateArchiveStorage + Send + 'static,
{
    /// Admit metadata, collect one bounded body, then dispatch storage away from the reactor.
    pub async fn serve(
        &self,
        request: Request<Incoming>,
    ) -> Result<Response<NativeHttpBody>, http::Error> {
        let head = request.method() == hyper::Method::HEAD;
        let (parts, body) = request.into_parts();
        let target = target(&parts);
        // reject method, target, or media type before polling or allocating for the body
        if let Err((status, error)) = admit_private_archive_transport(target.request()) {
            return response(refusal(status, error), head);
        }
        let body = match read_native_http_body(
            body,
            PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES,
            self.body_deadline,
        )
        .await
        {
            Ok(body) => body,
            // retain the existing closed rejection for deadline, transport, and size failures
            Err(_) => {
                return response(
                    refusal(
                        PrivateArchiveStatus::Conflict,
                        PrivateArchiveErrorBody::RevisionArchiveRejected,
                    ),
                    head,
                );
            }
        };
        let permit = match Arc::clone(&self.lane).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => return response(rejected(), head),
        };
        let storage = Arc::clone(&self.storage);
        // retain the permit for the complete synchronous archive operation and encoding pass
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut storage = match storage.lock() {
                Ok(storage) => storage,
                Err(_) => return response(rejected(), head),
            };
            let decoder = ArchiveWireCodec::new(NativeWirePrimitives);
            let mut encoder = ArchiveWireCodec::new(NativeWirePrimitives);
            let wire = serve_private_archive_wire_request(
                target.request(),
                &mut PrivateArchiveWireBody::new(&body, &decoder),
                &mut *storage,
                &mut encoder,
            );
            hyper_response(wire, head)
        })
        .await
        .unwrap_or_else(|_| response(rejected(), head))
    }

    /// Await the final admitted storage operation after the private listener stops accepting.
    pub async fn drain(&self) {
        // the sole permit is released only after storage and receipt encoding both settle
        let _permit = self.lane.acquire().await;
    }
}

/// Preserve only origin-form target bytes and a valid HTTP content-type representation.
fn target(parts: &http::request::Parts) -> NativePrivateArchiveTarget {
    // an absolute-form or authority-bearing target is outside the private route policy
    let target = if parts.uri.scheme().is_some() || parts.uri.authority().is_some() {
        String::new()
    } else {
        parts
            .uri
            .path_and_query()
            .map_or_else(String::new, |target| target.as_str().to_owned())
    };
    NativePrivateArchiveTarget {
        content_type: parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned),
        method: parts.method.as_str().to_owned(),
        target,
    }
}

/// Materialize one typed early refusal through the archive's actual wire owner.
fn refusal(status: PrivateArchiveStatus, error: PrivateArchiveErrorBody) -> EdgeWireResponse {
    materialize_private_archive_wire_response(
        PrivateArchiveResponse {
            status,
            body: PrivateArchiveResponseBody::Error(error),
        },
        &mut ArchiveWireCodec::new(NativeWirePrimitives),
    )
}

/// Preserve the source conflict returned when retained storage execution is unavailable.
fn rejected() -> EdgeWireResponse {
    refusal(
        PrivateArchiveStatus::Conflict,
        PrivateArchiveErrorBody::RevisionArchiveRejected,
    )
}

/// Convert one complete archive wire response to the shared native Hyper body.
fn response(wire: EdgeWireResponse, head: bool) -> Result<Response<NativeHttpBody>, http::Error> {
    hyper_response(wire, head)
}

/// Preserve source response headers while suppressing only an admitted HEAD representation.
fn hyper_response(
    mut wire: EdgeWireResponse,
    head: bool,
) -> Result<Response<NativeHttpBody>, http::Error> {
    // keep the representation metadata while omitting only the HEAD response body
    if head {
        wire.body = EdgeWireBody::Empty;
    }
    native_edge_http_head(
        wire.status_code,
        wire.header_profile,
        &wire.headers,
        wire.admin_cookie.as_ref(),
    )
    .body(NativeHttpBody::from_edge(wire.body))
}
