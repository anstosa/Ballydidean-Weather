#![allow(dead_code, unused_imports)]

// Reuse the actual edge dispatch and forecast-evidence owners without cloning their behavior.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../weather-edge/tests/support/edge_dispatch_fixture.rs"
));

// Native HTTP transport integration cases are added below this shared fixture.

use std::time::Duration;
use tokio::{net::TcpListener, runtime::Builder, sync::oneshot, task::LocalSet};
use weather_runtime::{
    NativeEdgeHttpRequest, NativeEdgeRequestAdmissions, NativeHttpListenerConfiguration,
    NativeHttpListenerReport, NativeWirePrimitives, native_edge_http_response,
    serve_native_edge_http1,
};

#[derive(Debug, PartialEq, Eq)]
enum NativeEdgeEvent {
    Route(String, String),
    Terminal(ForecastResponseTerminal, ForecastEvidenceOutcome),
}

struct NativeEdgeApplicationFixture {
    fixture: Fixture,
    admin: FixtureAdminSession,
    store: CapturedForecastRequests,
    json: FixtureWireJson,
    clock: DispatchCaptureClock,
    events: Vec<NativeEdgeEvent>,
}

/// Drive the actual edge route and settle each real socket event through existing evidence owners.
fn run_native_edge_fixture(
    client_mode: &str,
    vectored_writes: bool,
    projected_bytes: usize,
) -> (NativeHttpListenerReport, NativeEdgeApplicationFixture) {
    let fixture = Fixture::new();
    fixture.api.projected_forecast_bytes.set(projected_bytes);
    let app = Rc::new(RefCell::new(NativeEdgeApplicationFixture {
        fixture,
        admin: FixtureAdminSession,
        store: CapturedForecastRequests::default(),
        json: FixtureWireJson::default(),
        clock: DispatchCaptureClock,
        events: Vec::new(),
    }));
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let local = LocalSet::new();
    let report = local.block_on(&runtime, async {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, stopped) = oneshot::channel();
        let client_mode = client_mode.to_owned();
        let client = std::thread::spawn(move || {
            let result = Command::new("python3")
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/native-edge-http-client.py"),
                )
                .arg(client_mode)
                .arg(port.to_string())
                .arg(projected_bytes.to_string())
                .output();
            let _ = stop.send(());
            result
        });
        let app = Rc::clone(&app);
        let config = NativeHttpListenerConfiguration::new(
            2,
            Duration::from_secs(4),
            Duration::from_secs(15),
            Duration::from_millis(500),
            32 * 1024,
            vectored_writes,
        )
        .unwrap();
        let report = serve_native_edge_http1(
            listener,
            config,
            move |incoming, peer, scope| {
                let app = Rc::clone(&app);
                async move {
                    let input = NativeEdgeHttpRequest::read(incoming, peer, &NativeWirePrimitives)
                        .expect("ordinary parsed edge request");
                    let method = input.parts().method.as_str();
                    let pathname = &input.target().pathname;
                    let reply = {
                        let NativeEdgeApplicationFixture {
                            fixture,
                            admin,
                            store,
                            json,
                            events,
                            ..
                        } = &mut *app.borrow_mut();
                        events.push(NativeEdgeEvent::Route(
                            method.to_owned(),
                            pathname.to_owned(),
                        ));
                        // dispatch the actual incoming decoded metadata and ordinary public admissions
                        input
                            .dispatch(
                                RELEASE,
                                NativeEdgeRequestAdmissions::default(),
                                fixture.ports(admin),
                                EdgeForecastCaptureAdmission {
                                    channel_bound_capture_active: false,
                                    scheduled_capture: None,
                                },
                                store,
                                json,
                            )
                            .expect("ordinary edge fixture response")
                    };
                    native_edge_http_response(reply, scope, move |mut observer, terminal| {
                        let NativeEdgeApplicationFixture {
                            store,
                            clock,
                            events,
                            ..
                        } = &mut *app.borrow_mut();
                        // the real terminal owner now admits the original evidence persistence work
                        let work = observer
                            .observe(terminal, clock, store)
                            .expect("pending observer");
                        assert!(!observer.is_pending());
                        let outcome = persist_forecast_response_capture(
                            work,
                            store,
                            ForecastScheduleArchive::<DispatchCaptureArchive>::Disabled,
                        )
                        .evidence;
                        events.push(NativeEdgeEvent::Terminal(terminal, outcome));
                    })
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
    let app = Rc::try_unwrap(app)
        .ok()
        .expect("released connection owners")
        .into_inner();
    (report, app)
}

/// Verify pipelined mixed HEAD and GET actual socket ordering in both write strategies.
#[test]
fn native_edge_pipeline_preserves_real_forecast_terminals() {
    const FORECAST: &str = "/api/v1/sites/ballydidean/forecast";
    const CURRENT: &str = "/api/v1/sites/ballydidean/current";
    const WIDGET: &str = "/api/v1/sites/ballydidean/widget-forecast";
    const RECORDED: &str = "/api/v1/sites/ballydidean/forecast-observations";
    // execute the identical HTTP pipeline in queued and flattened write modes
    for vectored in [true, false] {
        let (report, app) = run_native_edge_fixture("mixed", vectored, 0);
        assert_eq!(report.accepted_connections, 1);
        assert_eq!(report.clean_connections, 1);
        assert_eq!(report.protocol_errors, 0);
        assert_eq!(
            (
                app.store.prepared.len(),
                app.store.commits,
                app.store.closes
            ),
            (2, 2, 0)
        );
        // the proxy receives the actual Accept field and the original encoded search
        assert_eq!(
            app.fixture
                .api
                .received_headers_and_search
                .borrow()
                .as_slice(),
            ["", "?days=%31", "", "?days=5", ""]
                .map(|search| ("application/json, text/plain".to_owned(), search.to_owned()))
        );
        // decoded query parameters select the same two exact predecessor evidence windows
        assert_eq!(
            app.store
                .prepared
                .iter()
                .map(|(_, window)| *window)
                .collect::<Vec<_>>(),
            [
                Some(ForecastEvidenceWindow::OneDay),
                Some(ForecastEvidenceWindow::FiveDays)
            ]
        );
        // bare query semantics differ between widget and recorded products in the predecessor
        assert_eq!(app.fixture.widget_client.calls.get(), 0);
        assert_eq!(app.fixture.recorded.calls.get(), 1);
        // each forecast commits before the following already-pipelined request routes
        assert_eq!(
            app.events,
            [
                NativeEdgeEvent::Route("HEAD".into(), FORECAST.into()),
                NativeEdgeEvent::Route("GET".into(), FORECAST.into()),
                NativeEdgeEvent::Terminal(
                    ForecastResponseTerminal::Finished,
                    ForecastEvidenceOutcome::Created
                ),
                NativeEdgeEvent::Route("GET".into(), WIDGET.into()),
                NativeEdgeEvent::Route("GET".into(), RECORDED.into()),
                NativeEdgeEvent::Route("GET".into(), CURRENT.into()),
                NativeEdgeEvent::Route("GET".into(), FORECAST.into()),
                NativeEdgeEvent::Terminal(
                    ForecastResponseTerminal::Finished,
                    ForecastEvidenceOutcome::Created
                ),
                NativeEdgeEvent::Route("HEAD".into(), FORECAST.into()),
            ]
        );
    }
}

/// Verify a real premature peer close cannot commit the projected forecast representation.
#[test]
fn native_edge_real_peer_close_aborts_forecast() {
    let (report, app) = run_native_edge_fixture("close", true, 16 * 1024 * 1024);
    assert_eq!(report.accepted_connections, 1);
    assert_eq!(
        (
            app.store.prepared.len(),
            app.store.commits,
            app.store.closes
        ),
        (1, 0, 1)
    );
    assert_eq!(
        app.events,
        [
            NativeEdgeEvent::Route("GET".into(), "/api/v1/sites/ballydidean/forecast".into()),
            NativeEdgeEvent::Terminal(
                ForecastResponseTerminal::Closed,
                ForecastEvidenceOutcome::ResponseAborted
            ),
        ]
    );
}
