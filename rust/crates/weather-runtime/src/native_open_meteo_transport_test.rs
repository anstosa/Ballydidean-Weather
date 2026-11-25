//! Loopback functional coverage for native Open-Meteo request binding.

use std::convert::Infallible;
use std::net::TcpListener as StdTcpListener;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::{ACCEPT, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use weather_edge::ArchiveWirePrimitives;
use weather_providers::{ProviderControls, ProviderRequestPolicy, run_provider_request};

use super::NativeOpenMeteoTransport;
use crate::{
    NativeWirePrimitives, OpenMeteoArchiveEndpoint, OpenMeteoArchiveQuery,
    OpenMeteoArchiveQueryValue, OpenMeteoArchiveTransport,
};

const ARCHIVE_BODY: &[u8] = br#"{"hourly":{"time":[]}}"#;
const ARCHIVE_BODY_SHA256: &str =
    "3746029ff8c68a4051cadf388da500c489564a40d85586702a3a70d8c94f6db1";

/// Serve one JSON response and capture the exact HTTP request target.
fn serve_archive_once() -> (String, mpsc::Receiver<String>, JoinHandle<()>) {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener.set_nonblocking(true).expect("set nonblocking");
    let address = listener.local_addr().expect("read loopback address");
    let (request_sender, request_receiver) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build loopback runtime");
        runtime.block_on(async move {
            let listener = TcpListener::from_std(listener).expect("admit loopback listener");
            let (socket, _) = listener.accept().await.expect("accept loopback request");
            let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                let request_sender = request_sender.clone();
                async move {
                    assert_eq!(
                        request.headers().get(ACCEPT),
                        Some(&HeaderValue::from_static("application/json"))
                    );
                    request_sender
                        .send(request.uri().to_string())
                        .expect("capture request target");
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(ARCHIVE_BODY))))
                }
            });
            http1::Builder::new()
                .keep_alive(false)
                .serve_connection(TokioIo::new(socket), service)
                .await
                .expect("serve loopback response");
        });
    });
    (format!("http://{address}"), request_receiver, server)
}

/// Preserve ordered URLSearchParams encoding and raw-body evidence over loopback HTTP.
#[test]
fn archive_binding_preserves_ordered_query_and_raw_digest() {
    let (origin, request_receiver, server) = serve_archive_once();
    let mut transport = NativeOpenMeteoTransport::new().expect("start native transport");
    let query = OpenMeteoArchiveQuery {
        endpoint: OpenMeteoArchiveEndpoint::Compatibility {
            origin: &origin,
            path: "/v1/archive",
        },
        parameters: [
            ("latitude", OpenMeteoArchiveQueryValue::Number(54.123)),
            ("longitude", OpenMeteoArchiveQueryValue::Number(-6.5)),
            (
                "hourly",
                OpenMeteoArchiveQueryValue::Text("temperature_2m,apparent_temperature"),
            ),
            (
                "temperature_unit",
                OpenMeteoArchiveQueryValue::Text("celsius"),
            ),
            ("wind_speed_unit", OpenMeteoArchiveQueryValue::Text("ms")),
            ("precipitation_unit", OpenMeteoArchiveQueryValue::Text("mm")),
            ("timezone", OpenMeteoArchiveQueryValue::Text("UTC")),
            ("start_date", OpenMeteoArchiveQueryValue::Text("2024-01-02")),
            ("end_date", OpenMeteoArchiveQueryValue::Text("2024-01-03")),
        ],
    };
    let mut exchange = transport
        .bind_archive(&query)
        .expect("bind Archive request");
    let response = run_provider_request(
        ProviderRequestPolicy {
            controls: ProviderControls {
                max_attempts: 1,
                max_body_bytes: 1_024,
                timeout_ms: 2_000,
            },
            deadline_at_ms: None,
        },
        &mut exchange,
    )
    .expect("complete Archive request");

    assert_eq!(response.status, 200);
    assert_eq!(response.body.checksum.as_str(), ARCHIVE_BODY_SHA256);
    assert_eq!(
        response.body.body,
        ArchiveWirePrimitives::decode_json(&NativeWirePrimitives, ARCHIVE_BODY)
            .expect("decode expected body")
    );
    assert_eq!(
        request_receiver.recv().expect("receive request target"),
        "/v1/archive?latitude=54.123&longitude=-6.5&hourly=temperature_2m%2Capparent_temperature&temperature_unit=celsius&wind_speed_unit=ms&precipitation_unit=mm&timezone=UTC&start_date=2024-01-02&end_date=2024-01-03"
    );
    drop(exchange);
    drop(transport);
    server.join().expect("join loopback server");
}
