//! Loopback functional coverage for the native Weather Underground transport.

use std::convert::Infallible;
use std::net::TcpListener as StdTcpListener;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::{ACCEPT, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use url::Url;
use weather_database::Sha256Digest;
use weather_providers::{ProviderControls, ProviderRequestPolicy};
use weather_worker::{
    ScheduledWindow, SupplementalPublicStationIdentity, SupplementalPublicStationScheduledRequest,
};

use super::NativeWundergroundTransport;
use crate::{NativeOutboundHttp, WorkerPublicStationEvidence, fetch_worker_wunderground};

const STATION_ID: &str = "KCASANFR123";
const PUBLIC_API_KEY: &str = "0123456789abcdef0123456789abcdef";
const COMBINED_DIGEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const RAW_DIGEST: &str = "79c42b2a42c3c38e860d44e51eb04acaef54b242c08c5db195555dfa4bd8c5f5";
const START_MS: i64 = 1_700_000_000_000;
const BODY: &[u8] = br#"{"observations":[{"epoch":1700000000,"stationID":"KCASANFR123","humidityAvg":50,"solarRadiationHigh":120,"uvHigh":3,"winddirAvg":360,"qcStatus":1,"metric":{"precipRate":1.5,"pressureMax":1001,"pressureMin":999,"tempAvg":12,"windgustHigh":36,"windspeedAvg":18}}]}"#;

/// Return one deterministic post-response receipt clock.
fn receipt_clock() -> Option<i64> {
    Some(START_MS + 60_000)
}

/// Capture the exact ordered raw-response checksum framing.
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

/// Serve one JSON response and retain the exact credentialed request target.
fn serve_wunderground_once() -> (Url, mpsc::Receiver<String>, JoinHandle<()>) {
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
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(BODY))))
                }
            });
            http1::Builder::new()
                .keep_alive(false)
                .serve_connection(TokioIo::new(socket), service)
                .await
                .expect("serve Weather Underground response");
        });
    });
    (
        Url::parse(&format!(
            "http://{address}/v2/pws/history/all?discarded=true"
        ))
        .expect("parse loopback endpoint"),
        request_receiver,
        server,
    )
}

/// Preserve local-calendar projection, query order, typed normalization, and digest framing.
#[test]
fn scheduled_transport_matches_weather_underground_contract() {
    let (endpoint, request_receiver, server) = serve_wunderground_once();
    let client = Arc::new(NativeOutboundHttp::new().expect("start native client"));
    let mut transport = NativeWundergroundTransport::with_endpoint(client, endpoint, receipt_clock);
    let mut evidence = Evidence::default();
    let request = SupplementalPublicStationScheduledRequest {
        identity: SupplementalPublicStationIdentity::WeatherUnderground {
            station_id: STATION_ID.to_owned(),
            public_api_key: PUBLIC_API_KEY.to_owned(),
        },
        model: Some("WS-2902".to_owned()),
        serial: Some("station-serial".to_owned()),
        source_id: "source-42".to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
        window: ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: START_MS + 60_000,
        },
    };
    let batch = fetch_worker_wunderground(
        &mut transport,
        &mut evidence,
        &request,
        STATION_ID,
        PUBLIC_API_KEY,
        ProviderRequestPolicy {
            controls: ProviderControls {
                max_attempts: 1,
                max_body_bytes: 4_096,
                timeout_ms: 2_000,
            },
            deadline_at_ms: None,
        },
    )
    .expect("complete Weather Underground request");

    assert_eq!(batch.attempts, 1);
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.records[0].received_at_ms, START_MS + 60_000);
    assert_eq!(batch.records[0].sample.metrics.temperature_c, Some(12.0));
    assert_eq!(batch.records[0].sample.metrics.wind_gust_mps, Some(10.0));
    assert_eq!(batch.records[0].sample.metrics.wind_speed_mps, Some(5.0));
    assert_eq!(batch.upstream_response_checksum, COMBINED_DIGEST);
    assert_eq!(evidence.material, format!("{RAW_DIGEST}\n").as_bytes());
    assert_eq!(
        request_receiver
            .recv()
            .expect("receive Weather Underground target"),
        "/v2/pws/history/all?stationId=KCASANFR123&format=json&units=m&numericPrecision=decimal&date=20231114&apiKey=0123456789abcdef0123456789abcdef"
    );
    drop(transport);
    server.join().expect("join loopback server");
}
