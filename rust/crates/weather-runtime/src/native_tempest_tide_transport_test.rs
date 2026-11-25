//! Loopback functional coverage for the native Tempest transport binding.

use std::convert::Infallible;
use std::net::TcpListener as StdTcpListener;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::{ACCEPT, HeaderValue, ORIGIN, USER_AGENT};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use url::Url;
use weather_providers::{ProviderControls, ProviderRequestPolicy, run_provider_request};
use weather_worker::TempestObservationRequest;

use super::{NativeTempestTideTransport, TEMPEST_REQUEST_ORIGIN, TEMPEST_USER_AGENT};
use crate::{NativeOutboundHttp, WorkerTempestTransport};

const TEMPEST_BODY: &[u8] = br#"{
  "status":{"status_code":0},
  "type":"obs_st",
  "device_id":123,
  "bucket_step_minutes":1,
  "obs":[[1700000000,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null,null]]
}"#;

/// Serve one Tempest response and retain the exact request target and public headers.
fn serve_tempest_once() -> (String, mpsc::Receiver<String>, JoinHandle<()>) {
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
                    assert_eq!(
                        request.headers().get(ORIGIN),
                        Some(&HeaderValue::from_static(TEMPEST_REQUEST_ORIGIN))
                    );
                    assert_eq!(
                        request.headers().get(USER_AGENT),
                        Some(&HeaderValue::from_static(TEMPEST_USER_AGENT))
                    );
                    request_sender
                        .send(request.uri().to_string())
                        .expect("capture request target");
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(TEMPEST_BODY))))
                }
            });
            http1::Builder::new()
                .keep_alive(false)
                .serve_connection(TokioIo::new(socket), service)
                .await
                .expect("serve Tempest response");
        });
    });
    (format!("http://{address}"), request_receiver, server)
}

/// Preserve the Tempest query, browser headers, and retained native payload projection.
#[test]
fn tempest_binding_uses_exact_query_headers_and_native_decoder() {
    let (origin, request_receiver, server) = serve_tempest_once();
    let client = Arc::new(NativeOutboundHttp::new().expect("start native client"));
    let tempest_origin = Url::parse(&origin).expect("parse Tempest fixture origin");
    let noaa_endpoint = tempest_origin
        .join("/api/prod/datagetter")
        .expect("build NOAA fixture endpoint");
    let mut transport = NativeTempestTideTransport::with_endpoints(
        client,
        "test-key".to_owned(),
        tempest_origin,
        noaa_endpoint,
    );
    let request = TempestObservationRequest {
        device_id: 123,
        end_exclusive_ms: 1_700_000_061_000,
        location_id: 456,
        serial: "ST-123".to_owned(),
        source_id: "tempest-123".to_owned(),
        start_ms: 1_700_000_000_000,
        timezone: "UTC".to_owned(),
    };
    let mut exchange = WorkerTempestTransport::bind_request(&mut transport, &request)
        .expect("bind Tempest request");
    let response = run_provider_request(
        ProviderRequestPolicy {
            controls: ProviderControls {
                max_attempts: 1,
                max_body_bytes: 4_096,
                timeout_ms: 2_000,
            },
            deadline_at_ms: None,
        },
        &mut exchange,
    )
    .expect("complete Tempest request");

    assert_eq!(response.body.envelope.device_id, 123);
    assert_eq!(
        response
            .body
            .envelope
            .observations
            .as_ref()
            .expect("retain observations")
            .len(),
        1
    );
    assert_eq!(
        request_receiver.recv().expect("receive Tempest target"),
        "/swd/rest/observations/device/123?api_key=test-key&build=169&time_start=1700000000&time_end=1700000060"
    );
    drop(exchange);
    drop(transport);
    server.join().expect("join loopback server");
}
