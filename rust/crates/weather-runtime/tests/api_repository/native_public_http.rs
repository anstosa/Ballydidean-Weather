//! Actual native public API ingress through the prepared fixture and production Rust URL/JSON primitives.

use super::{
    FixtureClock, SIDECAR_AS_OF, UnusedBoundary,
    api_repository_startup_fixture::start_prepared_api_fixture_with_checkouts,
};
use std::{cell::RefCell, path::PathBuf, process::Command, rc::Rc, time::Duration};
use tokio::{net::TcpListener, runtime::Builder, sync::oneshot, task::LocalSet};
use weather_api::{InactiveForecastServiceRuntime, WeatherServiceBoundaries};
use weather_runtime::{
    NativeHttpListenerConfiguration, NativePublicApiHttpRequest, NativeWeatherServiceWireEncoder,
    NativeWirePrimitives, serve_native_http1,
};

/// Drive real public HTTP routing, generated DTO serialization and the retained query pool.
#[test]
fn native_public_http_routes_and_query_parameters_match_node_boundary() {
    let prepared = start_prepared_api_fixture_with_checkouts(3);
    let app = Rc::new(RefCell::new(
        prepared.listeners.public.expect("public application"),
    ));
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let local = LocalSet::new();
    let report = local.block_on(&runtime, async {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, stopped) = oneshot::channel();
        let client = std::thread::spawn(move || {
            let output = Command::new("node")
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/native-api-http-client.mjs"),
                )
                .arg("public")
                .arg(port.to_string())
                .output();
            let _ = stop.send(());
            output
        });
        let configuration = NativeHttpListenerConfiguration::new(
            2,
            Duration::from_secs(4),
            Duration::from_secs(15),
            Duration::from_secs(1),
            32 * 1024,
            true,
        )
        .unwrap();
        let report = serve_native_http1(
            listener,
            configuration,
            move |incoming, _peer| {
                let app = Rc::clone(&app);
                async move {
                    let head = incoming.method() == hyper::Method::HEAD;
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
                    // route the decoded original target through the prepared API and real DTO encoder
                    request.serve(
                        &mut *app.borrow_mut(),
                        &mut FixtureClock,
                        &boundaries,
                        &mut NativeWeatherServiceWireEncoder::new(NativeWirePrimitives),
                    )
                }
            },
            async {
                let _ = stopped.await;
            },
        )
        .await
        .unwrap();
        let output = client.join().unwrap().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        report
    });
    assert_eq!(report.accepted_connections, 2);
    assert_eq!(report.clean_connections, 2);
    assert_eq!(report.protocol_errors, 0);
    // the two actual successful discovery responses and duplicate-query forecast read hit the retained pool
    let queries = prepared.pool_events.borrow().iter().filter(|event| {
        matches!(event, super::ApiPoolEvent::Query(sql, 0) if *sql == weather_database::LIST_ACTIVE_SITES_SQL)
    }).count();
    assert_eq!(queries, 3);
}
