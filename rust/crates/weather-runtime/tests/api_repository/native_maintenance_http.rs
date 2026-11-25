//! Actual private maintenance ingress over the prepared API application.

use std::cell::Cell;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::rc::Rc;
use std::time::Duration;

use tokio::{net::TcpListener, runtime::Builder, sync::oneshot, task::LocalSet};
use weather_runtime::{
    EncodedApiMaintenanceRequestOwners, HttpApiMaintenanceArchiveRelay,
    NativeAdjustmentMaintenanceWireEncoder, NativeApiMaintenanceHttpRequest,
    NativeApiMaintenanceRequestBodyCodec, NativeHttpListenerConfiguration, NativeWirePrimitives,
    serve_native_http1,
};

use super::api_repository_startup_fixture::{
    maintenance_capture_fixture::{CAPTURE_JSON, Capture, NoArchive, Values},
    start_prepared_api_fixture,
};

const CAPTURE_PATH: &str = "/internal/adjustment-maintenance/shadow/capture";
const SCHEDULE_PATH: &str = "/internal/adjustment-maintenance/schedule-slots";

/// Read one close-delimited HTTP response from a disposable test connection.
fn exchange(port: u16, request: &[u8]) -> Vec<u8> {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    socket.write_all(request).unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).unwrap();
    response
}

/// Split one complete HTTP response after its final header delimiter.
fn response_body(response: &[u8]) -> &[u8] {
    response
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .map(|index| &response[index + 4..])
        .expect("complete HTTP response head")
}

/// Prove valid capture, schedule dispatch, and pre-body raw-target refusal on real sockets.
#[test]
fn native_maintenance_ingress_preserves_private_request_contract() {
    let prepared = start_prepared_api_fixture();
    let maintenance = Rc::new(
        prepared
            .listeners
            .maintenance
            .expect("prepared private maintenance application"),
    );
    let captures = Rc::new(Cell::new(0));
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let local = LocalSet::new();
    let report = local.block_on(&runtime, async {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, stopped) = oneshot::channel();
        let client = std::thread::spawn(move || {
            let capture = exchange(
                port,
                format!(
                    "POST {CAPTURE_PATH} HTTP/1.1\r\nHost: api\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    CAPTURE_JSON.len(),
                    std::str::from_utf8(CAPTURE_JSON).unwrap(),
                )
                .as_bytes(),
            );
            assert!(capture.starts_with(b"HTTP/1.1 202 Accepted\r\n"), "{}", String::from_utf8_lossy(&capture));
            assert_eq!(response_body(&capture), br#"{"accepted":true}"#);

            let schedule = exchange(
                port,
                format!(
                    "GET {SCHEDULE_PATH} HTTP/1.1\r\nHost: api\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            );
            assert!(schedule.starts_with(b"HTTP/1.1 200 OK\r\n"), "{}", String::from_utf8_lossy(&schedule));
            assert!(schedule.windows(32).any(|bytes| bytes
                == b"content-type: application/json\r\n"));
            let schedule_text = std::str::from_utf8(response_body(&schedule)).unwrap();
            let temperature = schedule_text.find(r#""family":"temperature""#).unwrap();
            let wind = schedule_text.find(r#""family":"wind""#).unwrap();
            let rain = schedule_text.find(r#""family":"rain""#).unwrap();
            assert!(temperature < wind && wind < rain);

            // Send headers only: a wrong raw target must return without waiting for its body.
            let rejected = exchange(
                port,
                b"POST /internal/not-maintenance HTTP/1.1\r\nHost: api\r\nContent-Length: 8\r\nConnection: close\r\n\r\n",
            );
            assert!(rejected.starts_with(
                b"HTTP/1.1 404 Not Found\r\n"
            ));
            assert_eq!(response_body(&rejected), br#"{"error":"not_found"}"#);
            let _ = stop.send(());
        });
        let app = Rc::clone(&maintenance);
        let observed_captures = Rc::clone(&captures);
        let configuration = NativeHttpListenerConfiguration::new(
            2,
            Duration::from_secs(4),
            Duration::from_secs(12),
            Duration::from_secs(1),
            32 * 1024,
            true,
        )
        .unwrap();
        let report = serve_native_http1(
            listener,
            configuration,
            move |request, _peer| {
                let app = Rc::clone(&app);
                let observed_captures = Rc::clone(&observed_captures);
                async move {
                    let input =
                        NativeApiMaintenanceHttpRequest::read(request, Duration::from_secs(3))
                            .await;
                    let origin = app
                        .capture_material()
                        .0
                        .configuration
                        .archive_origin
                        .clone();
                    let mut relay = HttpApiMaintenanceArchiveRelay::new(origin, NoArchive);
                    let mut capture = Capture::default();
                    let response = input.serve(
                        &app,
                        EncodedApiMaintenanceRequestOwners {
                            body_codec: &mut NativeApiMaintenanceRequestBodyCodec::new(
                                NativeWirePrimitives,
                            ),
                            evidence_values: &Values,
                            capture: Some(&mut capture),
                            relay: &mut relay,
                        },
                        &mut NativeAdjustmentMaintenanceWireEncoder::new(NativeWirePrimitives),
                    )?;
                    // Only the admitted capture route can reach this callback.
                    if capture.0.is_some() {
                        observed_captures.set(observed_captures.get() + 1);
                    }
                    Ok::<_, hyper::http::Error>(response)
                }
            },
            async {
                let _ = stopped.await;
            },
        )
        .await
        .unwrap();
        client.join().unwrap();
        report
    });
    assert_eq!(captures.get(), 1);
    assert_eq!(report.accepted_connections, 3);
    assert_eq!(report.clean_connections, 3);
    assert_eq!(report.protocol_errors, 0);
}
