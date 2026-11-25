//! Loopback functional coverage for the native Netatmo transport.

use std::convert::Infallible;
use std::net::TcpListener as StdTcpListener;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use http_body_util::{BodyExt as _, Full};
use hyper::body::Bytes;
use hyper::header::{ACCEPT, AUTHORIZATION, CONNECTION, CONTENT_TYPE, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use url::Url;
use weather_database::Sha256Digest;
use weather_providers::{ProviderControls, ProviderRequestPolicy};
use weather_worker::{
    ScheduledWindow, SupplementalPublicStationIdentity, SupplementalPublicStationScheduledRequest,
};

use super::NativeNetatmoTransport;
use crate::{
    NativeOutboundHttp, NetatmoWorkerIdentity, WorkerPublicStationEvidence, fetch_worker_netatmo,
};

const DEVICE_ID: &str = "70:ee:50:71:3f:c4";
const OUTDOOR_ID: &str = "02:00:00:69:34:4e";
const RAIN_ID: &str = "05:00:00:08:97:7e";
const WIND_ID: &str = "06:00:00:06:45:7c";
const TOKEN: &str = "fixture-visitor-token";
const COMBINED_DIGEST: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const ORDERED_RESPONSE_DIGESTS: &str = concat!(
    "92eeb70340d5b207468236bf550c486732a53c1bc0b911982603c2d45e0d3b99\n",
    "2947ecafc5bb8ab54f5133591e21fbcbbda3cff48d5f4601c8c6dfe83134310f\n",
    "379ccd3e91a4f53aacf9ec7ba8ba246467b1a7ade05b8dcb51e754653ae12309\n",
    "57af44653f7156b4e300833fd4957fa1a6bf36e36cf8503efcdb35a35ada1895\n",
    "2ee8586b270e67cceaa47045da9cac5978af7e3a7754c5028eb41d8808fada3c\n",
);
const START_MS: i64 = 1_787_576_400_000;
const END_MS: i64 = 1_787_580_000_000;
const RECEIVED_MS: i64 = 1_787_583_600_000;

/// One captured loopback request without retaining runtime-owned header values.
struct CapturedRequest {
    body: String,
    path: String,
}

/// Return one deterministic receipt clock after every module settles.
fn receipt_clock() -> Option<i64> {
    Some(RECEIVED_MS)
}

/// Capture exact ordered upstream checksum framing.
#[derive(Default)]
struct Evidence {
    material: Vec<u8>,
}

impl WorkerPublicStationEvidence for Evidence {
    /// Retain digest input while returning one admitted fixture identity.
    fn sha256(&mut self, material: &[u8]) -> Result<Sha256Digest, weather_worker::TaskFailure> {
        self.material = material.to_vec();
        Ok(Sha256Digest::admitted(COMBINED_DIGEST))
    }
}

/// Serve the token and four independent module requests on one loopback origin.
fn serve_netatmo(fail_outdoor: bool) -> (Url, mpsc::Receiver<CapturedRequest>, JoinHandle<()>) {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind loopback");
    listener.set_nonblocking(true).expect("set nonblocking");
    let address = listener.local_addr().expect("read loopback address");
    let (capture_sender, capture_receiver) = mpsc::sync_channel(5);
    let server = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build loopback runtime");
        runtime.block_on(async move {
            let listener = TcpListener::from_std(listener).expect("admit loopback listener");
            let mut connections = tokio::task::JoinSet::new();
            // accept one close-delimited connection for each exact provider request
            for _ in 0..5 {
                let (socket, _) = listener.accept().await.expect("accept loopback request");
                let capture_sender = capture_sender.clone();
                connections.spawn(async move {
                    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                        let capture_sender = capture_sender.clone();
                        async move {
                            assert_eq!(
                                request.headers().get(ACCEPT),
                                Some(&HeaderValue::from_static("application/json"))
                            );
                            let path = request.uri().path().to_owned();
                            let is_token = path == "/weathermap/token";
                            // require credentials only on measurement requests
                            if is_token {
                                assert_eq!(request.method(), Method::GET);
                                assert!(request.headers().get(AUTHORIZATION).is_none());
                            } else {
                                assert_eq!(request.method(), Method::POST);
                                assert_eq!(
                                    request.headers().get(AUTHORIZATION),
                                    Some(
                                        &HeaderValue::from_str(&format!("Bearer {TOKEN}"))
                                            .expect("build fixture authorization")
                                    )
                                );
                                assert_eq!(
                                    request.headers().get(CONTENT_TYPE),
                                    Some(&HeaderValue::from_static("application/json"))
                                );
                            }
                            let body = request
                                .into_body()
                                .collect()
                                .await
                                .expect("read request body")
                                .to_bytes();
                            let body =
                                String::from_utf8(body.to_vec()).expect("decode request body");
                            capture_sender
                                .send(CapturedRequest {
                                    body: body.clone(),
                                    path,
                                })
                                .expect("capture Netatmo request");
                            let is_outdoor =
                                body.contains(&format!("\"module_id\":\"{OUTDOOR_ID}\""));
                            // keep the first configured failure ahead of slower successful siblings
                            if fail_outdoor && !is_token && !is_outdoor {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                            let response = response_body(is_token, &body);
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(if fail_outdoor && is_outdoor { 400 } else { 200 })
                                    .header(CONNECTION, "close")
                                    .body(Full::new(Bytes::from_static(response)))
                                    .expect("build loopback response"),
                            )
                        }
                    });
                    http1::Builder::new()
                        .keep_alive(false)
                        .serve_connection(TokioIo::new(socket), service)
                        .await
                        .expect("serve Netatmo response");
                });
            }
            // drain every response connection before stopping the fixture runtime
            while connections.join_next().await.is_some() {}
        });
    });
    (
        Url::parse(&format!("http://{address}")).expect("parse loopback origin"),
        capture_receiver,
        server,
    )
}

/// Select one typed response from the exact requested module identity.
fn response_body(token: bool, body: &str) -> &'static [u8] {
    // return the standalone visitor-token document before any module request
    if token {
        return br#"{"body":"fixture-visitor-token"}"#;
    }
    // route each closed request body to its matching one-row module payload
    if body.contains(&format!("\"module_id\":\"{OUTDOOR_ID}\"")) {
        br#"{"status":"ok","body":{"1787576617":[12.7,96]}}"#
    } else if body.contains(&format!("\"module_id\":\"{DEVICE_ID}\"")) {
        br#"{"status":"ok","body":{"1787576661":[1018.5]}}"#
    } else if body.contains(&format!("\"module_id\":\"{RAIN_ID}\"")) {
        br#"{"status":"ok","body":{"1787576655":[0.2]}}"#
    } else {
        assert!(body.contains(&format!("\"module_id\":\"{WIND_ID}\"")));
        br#"{"status":"ok","body":{"1787576655":[3,-1,5,999]}}"#
    }
}

/// Preserve token-first transport, concurrent exact bodies, typed units, and checksum order.
#[test]
fn netatmo_transport_executes_exact_token_and_module_contracts() {
    let (origin, capture_receiver, server) = serve_netatmo(false);
    let token_endpoint = origin
        .join("/weathermap/token?discarded=true")
        .expect("build token endpoint");
    let measure_endpoint = origin
        .join("/api/getmeasure?discarded=true")
        .expect("build measure endpoint");
    let client = Arc::new(NativeOutboundHttp::new().expect("start native client"));
    let mut transport = NativeNetatmoTransport::with_endpoints(
        client,
        token_endpoint,
        measure_endpoint,
        receipt_clock,
    );
    let mut evidence = Evidence::default();
    let request = fixture_request();
    let batch = fetch_worker_netatmo(
        &mut transport,
        &mut evidence,
        &request,
        fixture_identity(),
        ProviderRequestPolicy {
            controls: ProviderControls {
                max_attempts: 1,
                max_body_bytes: 4_096,
                timeout_ms: 2_000,
            },
            deadline_at_ms: None,
        },
    )
    .expect("complete Netatmo request");

    assert_eq!(batch.attempts, 5);
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.records[0].received_at_ms, RECEIVED_MS);
    assert_eq!(batch.records[0].sample.metrics.temperature_c, Some(12.7));
    assert_eq!(
        batch.records[0].sample.metrics.relative_humidity_percent,
        Some(96.0)
    );
    assert_eq!(batch.records[0].sample.metrics.pressure_hpa, Some(1_018.5));
    assert_eq!(batch.records[0].sample.metrics.precipitation_mm, Some(0.2));
    assert_eq!(
        batch.records[0].sample.metrics.wind_speed_mps,
        Some(3.0 / 3.6)
    );
    assert_eq!(batch.records[0].sample.metrics.wind_direction_degrees, None);
    assert_eq!(
        batch.records[0].sample.metrics.wind_gust_mps,
        Some(5.0 / 3.6)
    );
    assert_eq!(batch.upstream_response_checksum, COMBINED_DIGEST);

    let captures = (0..5)
        .map(|_| capture_receiver.recv().expect("receive Netatmo request"))
        .collect::<Vec<_>>();
    assert_eq!(
        captures
            .iter()
            .filter(|request| request.path == "/weathermap/token")
            .count(),
        1
    );
    let bodies = captures
        .iter()
        .filter(|request| request.path == "/api/getmeasure")
        .map(|request| request.body.as_str())
        .collect::<Vec<_>>();
    assert_eq!(bodies.len(), 4);
    assert!(bodies.contains(&module_body(OUTDOOR_ID, "Temperature,Humidity").as_str()));
    assert!(bodies.contains(&module_body(DEVICE_ID, "Pressure").as_str()));
    assert!(bodies.contains(&module_body(RAIN_ID, "Rain").as_str()));
    assert!(
        bodies.contains(
            &module_body(WIND_ID, "WindStrength,WindAngle,GustStrength,GustAngle").as_str()
        )
    );
    assert_eq!(evidence.material, ORDERED_RESPONSE_DIGESTS.as_bytes());
    drop(transport);
    server.join().expect("join loopback server");
}

/// Return the first completed module failure only after all four requests have drained.
#[test]
fn netatmo_transport_drains_module_siblings_after_first_failure() {
    let (origin, capture_receiver, server) = serve_netatmo(true);
    let token_endpoint = origin
        .join("/weathermap/token")
        .expect("build token endpoint");
    let measure_endpoint = origin
        .join("/api/getmeasure")
        .expect("build measure endpoint");
    let client = Arc::new(NativeOutboundHttp::new().expect("start native client"));
    let mut transport = NativeNetatmoTransport::with_endpoints(
        client,
        token_endpoint,
        measure_endpoint,
        receipt_clock,
    );
    let mut evidence = Evidence::default();
    let failure = fetch_worker_netatmo(
        &mut transport,
        &mut evidence,
        &fixture_request(),
        fixture_identity(),
        ProviderRequestPolicy {
            controls: ProviderControls {
                max_attempts: 1,
                max_body_bytes: 4_096,
                timeout_ms: 2_000,
            },
            deadline_at_ms: None,
        },
    )
    .expect_err("outdoor client response must fail the module cohort");

    assert_eq!(failure.code, "provider_request_rejected");
    assert_eq!(failure.attempts, 1);
    let captures = (0..5)
        .map(|_| capture_receiver.recv().expect("receive drained request"))
        .collect::<Vec<_>>();
    assert_eq!(
        captures
            .iter()
            .filter(|request| request.path == "/api/getmeasure")
            .count(),
        4
    );
    assert!(evidence.material.is_empty());
    drop(transport);
    server.join().expect("join loopback server");
}

/// Build the exact compact JSON body expected for one module request.
fn module_body(module_id: &str, measurement_types: &str) -> String {
    format!(
        "{{\"date_begin\":\"1787576400\",\"date_end\":\"1787579999\",\"device_id\":\"{DEVICE_ID}\",\"module_id\":\"{module_id}\",\"optimize\":false,\"scale\":\"max\",\"type\":\"{measurement_types}\"}}"
    )
}

/// Build one exact admitted scheduled Netatmo request.
fn fixture_request() -> SupplementalPublicStationScheduledRequest {
    SupplementalPublicStationScheduledRequest {
        identity: SupplementalPublicStationIdentity::Netatmo {
            device_id: DEVICE_ID.to_owned(),
            outdoor_module_id: OUTDOOR_ID.to_owned(),
            rain_module_id: RAIN_ID.to_owned(),
            wind_module_id: WIND_ID.to_owned(),
        },
        model: Some("test-model".to_owned()),
        serial: Some("test-serial".to_owned()),
        source_id: "source-public".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
        window: ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: END_MS,
        },
    }
}

/// Return the catalog-bound device and module identities.
const fn fixture_identity() -> NetatmoWorkerIdentity<'static> {
    NetatmoWorkerIdentity {
        device_id: DEVICE_ID,
        outdoor_module_id: OUTDOOR_ID,
        rain_module_id: RAIN_ID,
        wind_module_id: WIND_ID,
    }
}
