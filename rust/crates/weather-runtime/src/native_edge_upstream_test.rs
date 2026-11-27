//! Ordinary loopback coverage for the actual edge and maintenance transport adapters.

use crate::{
    ApiMaintenanceArchiveOriginOwner, ApiMaintenanceArchivePostTransport,
    ApiMaintenanceArchiveRawPost, NativeEdgeOrigins, NativeOutboundHttp,
    NativePrivatePostTransport, NativePublicApiProxy, native_edge_http_head,
};
use http_body_util::{BodyExt, Full};
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Incoming},
    header::{ACCEPT, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue, LOCATION},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::borrow::Cow;
use std::time::Duration;
use std::{convert::Infallible, net::TcpListener as StdTcpListener, sync::Arc, thread};
use weather_edge::{
    ApiUpstreamMethod, PublicApiProjection, PublicApiProxyClient, PublicApiProxyClientRequest,
    PublicApiResponseProjector,
};
use weather_edge::{EdgeStartupOrigins, EdgeWireHeaders, PublicServingHeaderProfile};

/// Serve a single admitted loopback exchange through Hyper's actual response framing.
fn server<F, Future>(handler: F) -> (String, thread::JoinHandle<()>)
where
    F: Fn(Request<Incoming>) -> Future + Send + 'static,
    Future: std::future::Future<Output = Response<Full<Bytes>>> + 'static,
{
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let thread = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let service = service_fn(move |request| {
                let response = handler(request);
                async move { Ok::<_, Infallible>(response.await) }
            });
            http1::Builder::new()
                .keep_alive(false)
                .serve_connection(TokioIo::new(socket), service)
                .await
                .unwrap();
        });
    });
    (format!("http://{address}"), thread)
}

struct FixtureProjection;
impl PublicApiResponseProjector for FixtureProjection {
    type Error = std::io::Error;
    type Snapshot = Vec<u8>;

    /// Capture one fixture setting before an ordinary projected request.
    fn prepare(&self, projection: PublicApiProjection) -> Result<Self::Snapshot, Self::Error> {
        assert_eq!(projection, PublicApiProjection::Forecast);
        Ok(b"snapshot:".to_vec())
    }

    /// Render the retained fixture projection over the fetched response.
    fn project(
        &self,
        _: PublicApiProjection,
        snapshot: &Self::Snapshot,
        source: &[u8],
    ) -> Result<Vec<u8>, Self::Error> {
        let mut body = snapshot.clone();
        body.extend_from_slice(source);
        Ok(body)
    }
}

/// Retain the original edge's upstream manual redirect and actual query spelling.
#[test]
fn native_edge_proxy_retains_manual_redirect_and_query() {
    let (origin, server) = server(|request| async move {
        assert_eq!(request.method(), "GET");
        assert_eq!(
            request.uri().path_and_query().unwrap().as_str(),
            "/api/v1/sites/ballydidean/history?limit=1&source=12"
        );
        assert_eq!(request.headers()[ACCEPT], "application/json");
        Response::builder()
            .status(StatusCode::FOUND)
            .header(LOCATION, "/other")
            .header(CONTENT_TYPE, "text/plain")
            .body(Full::new(Bytes::from_static(b"redirect")))
            .unwrap()
    });
    let client = Arc::new(NativeOutboundHttp::new().unwrap());
    let origin = NativeEdgeOrigins.api_origin(&origin).unwrap();
    let proxy = NativePublicApiProxy::new(origin, client, FixtureProjection).unwrap();
    let result = proxy
        .fetch_api(PublicApiProxyClientRequest {
            accept: "application/json",
            maximum_response_bytes: 1024,
            pathname: "/api/v1/sites/ballydidean/history",
            projection: PublicApiProjection::None,
            search: "?limit=1&source=12",
            timeout_ms: 3000,
            upstream_method: ApiUpstreamMethod::Get,
        })
        .unwrap();
    assert_eq!(result.status_code, 302);
    assert_eq!(result.body, b"redirect");
    assert!(!result.successful);
    server.join().unwrap();
}

/// Preserve ordinary HEAD representation metadata and avoid decoding an absent encoded body.
#[test]
fn native_edge_proxy_head_retains_content_length() {
    let (origin, server) = server(|request| async move {
        assert_eq!(request.method(), "HEAD");
        Response::builder()
            .status(200)
            .header(CONTENT_LENGTH, "7")
            .header(CONTENT_ENCODING, "gzip")
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from_static(b"ignored")))
            .unwrap()
    });
    let client = Arc::new(NativeOutboundHttp::new().unwrap());
    let origin = NativeEdgeOrigins.api_origin(&origin).unwrap();
    let proxy = NativePublicApiProxy::new(origin, client, FixtureProjection).unwrap();
    let result = proxy
        .fetch_api(PublicApiProxyClientRequest {
            accept: "application/json",
            maximum_response_bytes: 1024,
            pathname: "/api/v1/sites",
            projection: PublicApiProjection::None,
            search: "",
            timeout_ms: 3000,
            upstream_method: ApiUpstreamMethod::Head,
        })
        .unwrap();
    assert_eq!(result.status_code, 200);
    assert_eq!(result.content_length, Some(7));
    assert!(result.body.is_empty());
    server.join().unwrap();
}

/// Preserve node fetch's latin-1 projection and response-writer header octets.
#[test]
fn native_edge_proxy_round_trips_content_type_octets() {
    // cover ordinary ascii, isolated obs-text, and bytes that also form valid utf-8
    for expected in [
        b"application/json".as_slice(),
        b"text/plain; title=\"\xe9\"".as_slice(),
        b"text/plain; title=\"\xc3\xa9\"".as_slice(),
    ] {
        let upstream = HeaderValue::from_bytes(expected).unwrap();
        let (origin, server) = server(move |_| {
            let upstream = upstream.clone();
            async move {
                Response::builder()
                    .status(200)
                    .header(CONTENT_TYPE, upstream)
                    .body(Full::new(Bytes::from_static(b"{}")))
                    .unwrap()
            }
        });
        let client = Arc::new(NativeOutboundHttp::new().unwrap());
        let origin = NativeEdgeOrigins.api_origin(&origin).unwrap();
        let proxy = NativePublicApiProxy::new(origin, client, FixtureProjection).unwrap();
        let result = proxy
            .fetch_api(PublicApiProxyClientRequest {
                accept: "application/json",
                maximum_response_bytes: 1024,
                pathname: "/api/v1/sites",
                projection: PublicApiProjection::None,
                search: "",
                timeout_ms: 3000,
                upstream_method: ApiUpstreamMethod::Get,
            })
            .unwrap();
        let content_type = result.content_type.unwrap();
        let headers = EdgeWireHeaders {
            allow: None,
            cache_control: Cow::Borrowed("no-store"),
            content_length: result.body.len(),
            content_type: Some(Cow::Owned(content_type.clone())),
            location: None,
            retry_after: None,
            tile_age_seconds: None,
            tile_cache: None,
            vary: None,
        };
        let response =
            native_edge_http_head(200, PublicServingHeaderProfile::Standard, &headers, None)
                .body(())
                .unwrap();

        assert_eq!(
            content_type.chars().map(u32::from).collect::<Vec<_>>(),
            expected
                .iter()
                .map(|byte| u32::from(*byte))
                .collect::<Vec<_>>(),
        );
        assert_eq!(response.headers()[CONTENT_TYPE].as_bytes(), expected);
        server.join().unwrap();
    }
}

/// Transfer archive bytes through the actual private POST transport and retain its capacity reply.
#[test]
fn native_private_archive_post_preserves_body_and_capacity_status() {
    let (origin, server) = server(|request| async move {
        assert_eq!(request.method(), "POST");
        assert_eq!(
            request.uri().path(),
            "/internal/adjustment-maintenance/archive/publish"
        );
        assert_eq!(request.headers()[CONTENT_TYPE], "application/json");
        let body = request.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, Bytes::from_static(b"{\"fixture\":true}"));
        Response::builder()
            .status(429)
            .header(CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from_static(b"{}")))
            .unwrap()
    });
    let client = Arc::new(NativeOutboundHttp::new().unwrap());
    let mut transport = NativePrivatePostTransport::new(client, Duration::from_secs(3));
    let origin = NativeEdgeOrigins
        .admit_maintenance_archive_origin(&origin)
        .unwrap();
    let response = ApiMaintenanceArchivePostTransport::post(
        &mut transport,
        ApiMaintenanceArchiveRawPost {
            origin: &origin,
            path: "/internal/adjustment-maintenance/archive/publish",
            content_type: "application/json",
            maximum_response_body_bytes: 1024,
            body: b"{\"fixture\":true}",
        },
    )
    .unwrap();
    assert_eq!(response.status, 429);
    assert_eq!(response.body, b"{}");
    server.join().unwrap();
}
