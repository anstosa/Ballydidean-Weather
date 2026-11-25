//! Loopback functional coverage for the native PurpleAir transport.

use std::convert::Infallible;
use std::net::TcpListener as StdTcpListener;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::{ACCEPT, CONNECTION, HeaderValue, ORIGIN, REFERER, USER_AGENT};
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
    TaskFailure,
};

use super::{
    NativePurpleAirTransport, PURPLEAIR_MAP_ORIGIN, PURPLEAIR_REFERER, PURPLEAIR_USER_AGENT,
};
use crate::{
    NativeOutboundHttp, WorkerPublicStationEvidence, WorkerPurpleAirOperation,
    WorkerPurpleAirTransport,
};

const TOKEN: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CSV: &str = concat!(
    "time_stamp,sensor_index,humidity_a,humidity_b,pm2.5_atm_a,pm2.5_atm_b,pressure_a,pressure_b,temperature_a,temperature_b\n",
    "2026-08-24T13:00:00Z,32489,50,null,10,12,1000,null,68,null"
);
const COMBINED_DIGEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const START_MS: i64 = 1_787_576_400_000;

/// Return one deterministic clock for receipt and any disabled fixture cache calls.
fn fixture_clock() -> Option<i64> {
    Some(START_MS + 2 * 60 * 60 * 1_000)
}

/// Capture the exact history-only checksum framing.
#[derive(Default)]
struct Evidence {
    material: Vec<u8>,
}

impl WorkerPublicStationEvidence for Evidence {
    /// Retain digest input while returning one admitted fixture digest.
    fn sha256(&mut self, material: &[u8]) -> Result<Sha256Digest, TaskFailure> {
        self.material = material.to_vec();
        Ok(Sha256Digest::admitted(COMBINED_DIGEST))
    }
}

/// One captured PurpleAir request without retaining secret material in diagnostics.
struct CapturedRequest {
    accept: Option<String>,
    origin: Option<String>,
    path: String,
    referer: Option<String>,
    token: Option<String>,
    user_agent: Option<String>,
}

/// Serve the public token and history responses on two finite loopback connections.
fn serve_purpleair() -> (Url, mpsc::Receiver<CapturedRequest>, JoinHandle<()>) {
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
            // serve the token and history requests as separate closed HTTP connections
            for index in 0..2 {
                let (socket, _) = listener.accept().await.expect("accept loopback request");
                let request_sender = request_sender.clone();
                let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                    let request_sender = request_sender.clone();
                    async move {
                        let header = |name: &str| {
                            request
                                .headers()
                                .get(name)
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_owned)
                        };
                        request_sender
                            .send(CapturedRequest {
                                accept: header(ACCEPT.as_str()),
                                origin: header(ORIGIN.as_str()),
                                path: request.uri().to_string(),
                                referer: header(REFERER.as_str()),
                                token: header("x-api-token"),
                                user_agent: header(USER_AGENT.as_str()),
                            })
                            .expect("capture PurpleAir request");
                        // serve the public token before the CSV history document
                        let body = if index == 0 { TOKEN } else { CSV };
                        let mut response =
                            Response::new(Full::new(Bytes::copy_from_slice(body.as_bytes())));
                        response
                            .headers_mut()
                            .insert(CONNECTION, HeaderValue::from_static("close"));
                        Ok::<_, Infallible>(response)
                    }
                });
                http1::Builder::new()
                    .keep_alive(false)
                    .serve_connection(TokioIo::new(socket), service)
                    .await
                    .expect("serve PurpleAir response");
            }
        });
    });
    (
        Url::parse(&format!("http://{address}/discarded?query=true"))
            .expect("parse loopback origin"),
        request_receiver,
        server,
    )
}

/// Preserve exact token/history wire contracts, typed CSV projection, and raw digest evidence.
#[test]
fn scheduled_transport_matches_purpleair_contract() {
    let (endpoint, request_receiver, server) = serve_purpleair();
    let client = Arc::new(NativeOutboundHttp::new().expect("start native client"));
    let transport = NativePurpleAirTransport::with_endpoint(client, endpoint, fixture_clock);
    assert!(!transport.purpleair_production_cache_enabled());
    let mut operation = WorkerPurpleAirOperation::new(transport);
    let mut evidence = Evidence::default();
    let request = SupplementalPublicStationScheduledRequest {
        identity: SupplementalPublicStationIdentity::PurpleAir {
            map_version: "1.2.3".to_owned(),
            sensor_index: 32_489,
        },
        model: Some("PA-II".to_owned()),
        serial: Some("purple-32489".to_owned()),
        source_id: "source-purpleair".to_owned(),
        timezone: "UTC".to_owned(),
        window: ScheduledWindow {
            start_ms: START_MS,
            end_exclusive_ms: START_MS + 60 * 60 * 1_000,
        },
    };
    let batch = operation
        .fetch(
            &mut evidence,
            &request,
            "1.2.3",
            32_489,
            ProviderRequestPolicy {
                controls: ProviderControls {
                    max_attempts: 1,
                    max_body_bytes: 4_096,
                    timeout_ms: 2_000,
                },
                deadline_at_ms: None,
            },
        )
        .expect("complete PurpleAir request");

    assert_eq!(batch.attempts, 2);
    assert_eq!(batch.records.len(), 1);
    assert_eq!(
        batch.records[0].received_at_ms,
        START_MS + 2 * 60 * 60 * 1_000
    );
    assert_eq!(
        batch.records[0]
            .sample
            .metrics
            .pm25_micrograms_per_cubic_meter,
        Some(11.0)
    );
    assert_eq!(batch.records[0].sample.metrics.pressure_hpa, Some(1_000.0));
    assert_eq!(
        batch.records[0].sample.metrics.relative_humidity_percent,
        Some(50.0)
    );
    assert_eq!(batch.records[0].sample.metrics.temperature_c, Some(20.0));
    assert_eq!(batch.upstream_response_checksum, COMBINED_DIGEST);

    let token_request = request_receiver.recv().expect("receive token request");
    let history_request = request_receiver.recv().expect("receive history request");
    assert_eq!(token_request.path, "/v1/token?version=1.2.3");
    assert_eq!(token_request.accept.as_deref(), Some("text/plain"));
    assert_eq!(token_request.origin.as_deref(), Some(PURPLEAIR_MAP_ORIGIN));
    assert_eq!(token_request.referer.as_deref(), Some(PURPLEAIR_REFERER));
    assert_eq!(
        token_request.user_agent.as_deref(),
        Some(PURPLEAIR_USER_AGENT)
    );
    assert_eq!(token_request.token, None);
    assert_eq!(
        history_request.path,
        "/v1/sensors/32489/history/csv?fields=humidity_a%2Chumidity_b%2Cpm2.5_atm_a%2Cpm2.5_atm_b%2Cpressure_a%2Cpressure_b%2Ctemperature_a%2Ctemperature_b&start_timestamp=2026-08-24T13%3A00%3A00.000Z&end_timestamp=2026-08-24T14%3A00%3A00.000Z&average=0"
    );
    assert_eq!(history_request.token.as_deref(), Some(TOKEN));
    assert_eq!(
        history_request.origin.as_deref(),
        Some(PURPLEAIR_MAP_ORIGIN)
    );
    assert_eq!(history_request.referer.as_deref(), Some(PURPLEAIR_REFERER));
    assert_eq!(
        history_request.user_agent.as_deref(),
        Some(PURPLEAIR_USER_AGENT)
    );
    assert_eq!(
        evidence.material,
        format!("{}\n", sha256_hex(CSV.as_bytes())).into_bytes()
    );
    drop(operation);
    server.join().expect("join loopback server");
}

/// Hash one finite fixture body with the same approved digest primitive as production.
fn sha256_hex(bytes: &[u8]) -> String {
    use weather_edge::RevisionArchiveCanonicalCodec as _;

    let digest = crate::NativeWirePrimitives.sha256(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    // emit the expected lowercase checksum framing
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
