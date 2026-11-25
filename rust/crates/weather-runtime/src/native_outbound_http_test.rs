//! Loopback functional coverage for the retained native outbound client.

use std::convert::Infallible;
use std::io::Write as _;
use std::net::TcpListener as StdTcpListener;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use flate2::Compression;
use flate2::write::GzEncoder;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::{ACCEPT_ENCODING, CONTENT_ENCODING, HeaderMap, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use url::Url;
use weather_providers::ContentBudgetRole;

use crate::native_outbound_http::{
    NativeOutboundHttp, NativeOutboundHttpError, NativeOutboundHttpRequest,
};

/// Serve one finite Hyper response and return its absolute loopback URL.
fn serve_once(status: StatusCode, headers: HeaderMap, body: Vec<u8>) -> (Url, JoinHandle<()>) {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener.set_nonblocking(true).expect("set nonblocking");
    let address = listener.local_addr().expect("read loopback address");
    let thread = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build loopback runtime");
        runtime.block_on(async move {
            let listener = TcpListener::from_std(listener).expect("admit loopback listener");
            let (socket, _) = listener.accept().await.expect("accept loopback request");
            let service = service_fn(move |request| {
                let headers = headers.clone();
                let body = body.clone();
                async move {
                    assert_eq!(
                        request.headers().get(ACCEPT_ENCODING),
                        Some(&HeaderValue::from_static("gzip, deflate"))
                    );
                    let mut response = Response::new(Full::new(Bytes::from(body)));
                    *response.status_mut() = status;
                    *response.headers_mut() = headers;
                    Ok::<_, Infallible>(response)
                }
            });
            http1::Builder::new()
                .keep_alive(false)
                .serve_connection(TokioIo::new(socket), service)
                .await
                .expect("serve loopback response");
        });
    });
    (
        Url::parse(&format!("http://{address}/provider")).expect("build loopback URL"),
        thread,
    )
}

/// Decode an advertised gzip body inside the full request deadline.
#[test]
fn outbound_http_decodes_bounded_gzip_response() {
    let expected = b"provider body";
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(expected).expect("compress fixture");
    let encoded = encoder.finish().expect("finish fixture");
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    let (url, server) = serve_once(StatusCode::OK, headers, encoded);
    let client = NativeOutboundHttp::new().expect("start native client");
    let response = client
        .execute(
            NativeOutboundHttpRequest::new(Method::GET, url).expect("admit request"),
            Duration::from_secs(2),
            1_024,
        )
        .expect("complete loopback request");

    assert_eq!(response.status, 200);
    assert_eq!(response.body, expected);
    assert_eq!(response.streamed_body_bytes, expected.len() as u64);
    drop(client);
    server.join().expect("join loopback server");
}

/// Stop body consumption at the configured provider byte ceiling.
#[test]
fn outbound_http_rejects_oversized_loopback_response() {
    let (url, server) = serve_once(StatusCode::OK, HeaderMap::new(), b"12345".to_vec());
    let client = NativeOutboundHttp::new().expect("start native client");
    let error = client
        .execute(
            NativeOutboundHttpRequest::new(Method::GET, url).expect("admit request"),
            Duration::from_secs(2),
            4,
        )
        .expect_err("reject oversized response");

    assert!(matches!(
        error,
        NativeOutboundHttpError::BodyTooLarge {
            role: ContentBudgetRole::DeclaredLength | ContentBudgetRole::StreamedBytes,
            status: 200,
        }
    ));
    drop(client);
    server.join().expect("join loopback server");
}

/// Prove that two independent provider exchanges can both reach a single retained runtime.
#[test]
fn outbound_http_runs_paired_exchanges_concurrently() {
    use std::sync::Arc;
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = TcpListener::from_std(listener).unwrap();
            let barrier = Arc::new(tokio::sync::Barrier::new(2));
            let mut connections = tokio::task::JoinSet::new();
            // keep the first response pending until the second request has actually arrived
            for _ in 0..2 {
                let (socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let barrier = Arc::clone(&barrier);
                connections.spawn(async move {
                    let service = service_fn(move |_| {
                        let barrier = Arc::clone(&barrier);
                        async move {
                            tokio::time::timeout(Duration::from_secs(3), barrier.wait())
                                .await
                                .unwrap();
                            Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(
                                b"paired",
                            ))))
                        }
                    });
                    http1::Builder::new()
                        .keep_alive(false)
                        .serve_connection(TokioIo::new(socket), service)
                        .await
                        .unwrap();
                });
            }
            // require both served connections to settle before the server exits
            while let Some(result) = connections.join_next().await {
                result.unwrap();
            }
        });
    });
    let client = NativeOutboundHttp::new().unwrap();
    let url = Url::parse(&format!("http://{address}/forecast")).unwrap();
    thread::scope(|scope| {
        let client = &client;
        let request_one = NativeOutboundHttpRequest::new(Method::GET, url.clone()).unwrap();
        let request_two = NativeOutboundHttpRequest::new(Method::GET, url.clone()).unwrap();
        let one = scope.spawn(move || client.execute(request_one, Duration::from_secs(4), 1024));
        let two = scope.spawn(move || client.execute(request_two, Duration::from_secs(4), 1024));
        assert_eq!(one.join().unwrap().unwrap().body, b"paired");
        assert_eq!(two.join().unwrap().unwrap().body, b"paired");
    });
    server.join().unwrap();
}
