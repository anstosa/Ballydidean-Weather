//! Loopback coverage for the two-stage native Ecowitt gateway transport.

use std::convert::Infallible;
use std::net::TcpListener as StdTcpListener;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use url::Url;
use weather_worker::{EcowittScheduledRequest, ScheduledProvider};

use super::NativeEcowittTransport;
use crate::{NativeOutboundHttp, RetryingEcowittProvider, WorkerEcowittTransport};

const IDENTITY_BODY: &[u8] = br#"{"mac":"AA:BB:CC:DD:EE:FF"}"#;
const LIVE_BODY: &[u8] = br#"{"common_list":[]}"#;

/// Serve the identity and live endpoints as two finite loopback connections.
fn serve_ecowitt_pair() -> (Url, mpsc::Receiver<String>, JoinHandle<()>) {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener.set_nonblocking(true).expect("set nonblocking");
    let address = listener.local_addr().expect("read loopback address");
    let (request_sender, request_receiver) = mpsc::sync_channel(2);
    let server = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build loopback runtime");
        runtime.block_on(async move {
            let listener = TcpListener::from_std(listener).expect("admit loopback listener");
            // serve the sequential identity and live controllers independently
            for _ in 0..2 {
                let (socket, _) = listener.accept().await.expect("accept loopback request");
                let request_sender = request_sender.clone();
                let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                    let request_sender = request_sender.clone();
                    async move {
                        let target = request.uri().path().to_owned();
                        request_sender
                            .send(target.clone())
                            .expect("capture Ecowitt target");
                        let body = if target == "/get_network_info" {
                            IDENTITY_BODY
                        } else {
                            LIVE_BODY
                        };
                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(body))))
                    }
                });
                http1::Builder::new()
                    .keep_alive(false)
                    .serve_connection(TokioIo::new(socket), service)
                    .await
                    .expect("serve Ecowitt response");
            }
        });
    });
    (
        Url::parse(&format!("http://{address}")).expect("build loopback origin"),
        request_receiver,
        server,
    )
}

/// Preserve stage order, identity binding, checksum framing, calendar, and metadata owners.
#[test]
fn ecowitt_transport_executes_identity_then_live_data() {
    let (origin, request_receiver, server) = serve_ecowitt_pair();
    let client = Arc::new(NativeOutboundHttp::new().expect("start native client"));
    let mut transport = NativeEcowittTransport::with_test_origin(client, origin);
    let request = EcowittScheduledRequest {
        expected_mac: "AA:BB:CC:DD:EE:FF".to_owned(),
        gateway_host: "192.168.1.20".to_owned(),
        model: "GW3000".to_owned(),
        previous_cursor: None,
        source_id: "ecowitt-gateway".to_owned(),
        timezone: "UTC".to_owned(),
    };
    let exchange = transport
        .bind_request(&request)
        .expect("bind Ecowitt exchange");
    let mut provider =
        RetryingEcowittProvider::new(exchange.io, None, exchange.time, exchange.metadata);
    let batch = ScheduledProvider::fetch(&mut provider, request)
        .expect("complete two-stage Ecowitt request");

    assert_eq!(batch.attempts, 2);
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.response_metadata.gateway_mac, "AA:BB:CC:DD:EE:FF");
    assert_eq!(batch.response_metadata.group_count, 1);
    assert_eq!(batch.upstream_response_checksum.len(), 64);
    assert_eq!(
        [
            request_receiver.recv().expect("receive identity target"),
            request_receiver.recv().expect("receive live target"),
        ],
        ["/get_network_info", "/get_livedata_info"]
    );
    drop(provider);
    drop(transport);
    server.join().expect("join loopback server");
}
