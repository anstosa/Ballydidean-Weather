//! Process-level ownership of both prepared API listeners over their actual HTTP adapters.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::num::NonZeroU16;
use std::rc::Rc;
use std::time::Duration;

use tokio::{runtime::Builder, sync::oneshot, task::LocalSet};
use weather_api::{InactiveForecastServiceRuntime, WeatherServiceBoundaries};
use weather_runtime::{
    EncodedApiMaintenanceRequestOwners, HttpApiMaintenanceArchiveRelay,
    NativeAdjustmentMaintenanceWireEncoder, NativeApiMaintenanceHttpRequest,
    NativeApiMaintenanceRequestBodyCodec, NativeApiPreparedListeners,
    NativeHttpListenerConfiguration, NativePublicApiHttpRequest, NativeWeatherServiceWireEncoder,
    NativeWirePrimitives, WeatherApiPreparedListeners,
};

use super::api_repository_startup_fixture::{
    maintenance_capture_fixture::{NoArchive, Values},
    start_prepared_api_fixture_with_checkouts,
};
use super::{FixtureClock, SIDECAR_AS_OF, UnusedBoundary};

const SCHEDULE_PATH: &str = "/internal/adjustment-maintenance/schedule-slots";

/// Reserve two distinct nonzero loopback ports until both assignments are known.
fn reserve_loopback_ports() -> (NonZeroU16, NonZeroU16) {
    let public = TcpListener::bind(("127.0.0.1", 0)).expect("public port reservation");
    let maintenance = TcpListener::bind(("127.0.0.1", 0)).expect("maintenance port reservation");
    let public_port = NonZeroU16::new(public.local_addr().unwrap().port()).unwrap();
    let maintenance_port = NonZeroU16::new(maintenance.local_addr().unwrap().port()).unwrap();
    (public_port, maintenance_port)
}

/// Exchange one close-delimited request with an already-bound process listener.
fn exchange(address: SocketAddr, request: &[u8]) -> Vec<u8> {
    let mut socket = TcpStream::connect(address).expect("bound API listener");
    socket
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    socket.write_all(request).unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).unwrap();
    response
}

/// Supply the process-level listener limits used by both fixture sockets.
fn listener_configuration() -> NativeHttpListenerConfiguration {
    NativeHttpListenerConfiguration::new(
        2,
        Duration::from_secs(4),
        Duration::from_secs(12),
        Duration::from_secs(1),
        32 * 1024,
        true,
    )
    .unwrap()
}

/// Prove one shutdown owns both real API sockets, applications, and listener reports.
#[test]
fn native_api_process_serves_and_stops_both_prepared_listeners() {
    let mut prepared = start_prepared_api_fixture_with_checkouts(2);
    let public = prepared
        .listeners
        .public
        .take()
        .expect("public application");
    let maintenance = prepared
        .listeners
        .maintenance
        .take()
        .expect("maintenance application");
    let (public_port, maintenance_port) = reserve_loopback_ports();
    let mut listeners =
        NativeApiPreparedListeners::loopback(listener_configuration(), listener_configuration());
    listeners.listen_public(public_port, public).unwrap();
    listeners
        .listen_maintenance(maintenance_port, maintenance)
        .unwrap();
    let public_address = listeners.public_local_addr().expect("public address");
    let maintenance_address = listeners
        .maintenance_local_addr()
        .expect("maintenance address");
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let local = LocalSet::new();
    let (report, public_address, maintenance_address) = local.block_on(&runtime, async move {
        let (stop, stopped) = oneshot::channel();
        // exercise both bound sockets before resolving their shared shutdown
        let client = std::thread::spawn(move || {
            let public_response = exchange(
                public_address,
                b"GET /api/v1/sites HTTP/1.1\r\nHost: api\r\nConnection: close\r\n\r\n",
            );
            assert!(
                public_response.starts_with(b"HTTP/1.1 200 OK\r\n"),
                "{}",
                String::from_utf8_lossy(&public_response)
            );
            let maintenance_response = exchange(
                maintenance_address,
                format!("GET {SCHEDULE_PATH} HTTP/1.1\r\nHost: api\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            );
            assert!(
                maintenance_response.starts_with(b"HTTP/1.1 200 OK\r\n"),
                "{}",
                String::from_utf8_lossy(&maintenance_response)
            );
            let _ = stop.send(());
        });
        let report = listeners
            .serve_with_maintenance(
                // adapt public traffic with the retained application owner
                |incoming, _peer, app: Rc<RefCell<_>>| async move {
                    let head = incoming.method() == hyper::Method::HEAD;
                    // return the outer ingress failure before service routing
                    let request =
                        match NativePublicApiHttpRequest::read(incoming, &NativeWirePrimitives) {
                            Ok(request) => request,
                            Err(error) => return error.response(head),
                        };
                    let boundary = UnusedBoundary;
                    let inactive =
                        InactiveForecastServiceRuntime::unconfigured(SIDECAR_AS_OF, SIDECAR_AS_OF);
                    let boundaries = WeatherServiceBoundaries {
                        nominal_data_at: None,
                        time_resolver: &boundary,
                        history: &boundary,
                        tides: &boundary,
                        forecast: &boundary,
                        forecast_runtime: &inactive,
                        forecast_wind_runtime: None,
                        forecast_temperature_runtime: None,
                        forecast_rain_runtime: None,
                        trends: &boundary,
                        daily_precipitation: &boundary,
                        rain_collection: &boundary,
                        forecast_observation: &boundary,
                        version: None,
                    };
                    // route through the actual public request target and wire owners
                    request.serve(
                        &mut *app.borrow_mut(),
                        &mut FixtureClock,
                        &boundaries,
                        &mut NativeWeatherServiceWireEncoder::new(NativeWirePrimitives),
                    )
                },
                // adapt private traffic with the retained application owner
                |incoming, _peer, app: Rc<_>| async move {
                    let request =
                        NativeApiMaintenanceHttpRequest::read(incoming, Duration::from_secs(3))
                            .await;
                    let origin = app
                        .capture_material()
                        .0
                        .configuration
                        .archive_origin
                        .clone();
                    let mut relay = HttpApiMaintenanceArchiveRelay::new(origin, NoArchive);
                    // route through the actual private request body and wire owners
                    request.serve(
                        &app,
                        EncodedApiMaintenanceRequestOwners {
                            body_codec: &mut NativeApiMaintenanceRequestBodyCodec::new(
                                NativeWirePrimitives,
                            ),
                            evidence_values: &Values,
                            capture: None,
                            relay: &mut relay,
                        },
                        &mut NativeAdjustmentMaintenanceWireEncoder::new(NativeWirePrimitives),
                    )
                },
                async {
                    let _ = stopped.await;
                },
            )
            .await
            .unwrap();
        client.join().unwrap();
        (report, public_address, maintenance_address)
    });
    assert_eq!(report.public.accepted_connections, 1);
    assert_eq!(report.public.clean_connections, 1);
    assert_eq!(report.public.protocol_errors, 0);
    let maintenance_report = report.maintenance.expect("maintenance report");
    assert_eq!(maintenance_report.accepted_connections, 1);
    assert_eq!(maintenance_report.clean_connections, 1);
    assert_eq!(maintenance_report.protocol_errors, 0);
    assert!(TcpStream::connect_timeout(&public_address, Duration::from_millis(250)).is_err());
    assert!(TcpStream::connect_timeout(&maintenance_address, Duration::from_millis(250)).is_err());
}

/// Prove a maintenance bind conflict releases a previously bound public socket.
#[test]
fn native_api_process_rolls_back_public_bind_after_maintenance_conflict() {
    let mut prepared = start_prepared_api_fixture_with_checkouts(1);
    let public = prepared
        .listeners
        .public
        .take()
        .expect("public application");
    let maintenance = prepared
        .listeners
        .maintenance
        .take()
        .expect("maintenance application");
    let public_reservation = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let public_port = NonZeroU16::new(public_reservation.local_addr().unwrap().port()).unwrap();
    let maintenance_conflict = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let maintenance_port =
        NonZeroU16::new(maintenance_conflict.local_addr().unwrap().port()).unwrap();
    drop(public_reservation);
    let mut listeners =
        NativeApiPreparedListeners::loopback(listener_configuration(), listener_configuration());
    listeners.listen_public(public_port, public).unwrap();
    assert!(
        listeners
            .listen_maintenance(maintenance_port, maintenance)
            .is_err()
    );
    let rebound = TcpListener::bind(("127.0.0.1", public_port.get()))
        .expect("failed maintenance bind rolls back public socket");
    assert_eq!(rebound.local_addr().unwrap().port(), public_port.get());
}
