#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#![allow(dead_code, unused_imports)]

// Reuse the actual edge dispatch owners instead of cloning their application behavior.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../weather-edge/tests/support/edge_dispatch_fixture.rs"
));

#[path = "support/node_primitives.rs"]
mod node_primitives;

use node_primitives::NodePrimitives;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::num::NonZeroU16;
use std::os::unix::fs::DirBuilderExt;
use std::time::Duration;
use tokio::sync::oneshot;
use weather_edge::{
    ArchiveWireCodec, EdgeStartupLifecycleConfiguration, ForecastSchedulerConfiguration,
    ForecastSchedulerPollTarget, ForecastSchedulerPublicReadyStarter,
};
use weather_runtime::{
    NativeBackendRuntime, NativeEdgeHttpRequest, NativeEdgePreparedProcess,
    NativeEdgePublicApplication, NativeEdgeRequestAdmissions, NativeEdgeStartupPorts,
    NativeEdgeStartupResources, NativeForecastSchedulerRuntime, NativeHttpListenerConfiguration,
    NativePrivateArchiveHttpRequest, NativeWirePrimitives, bind_native_private_archive_stores,
    native_edge_http_response,
};

const PRIVATE_STATUS_PATH: &str =
    "/internal/adjustment-maintenance/rain-fixed-gauge-target/gap/status";

#[derive(Clone, Copy)]
struct FixtureClock;

impl RevisionArchiveCanonicalClock for FixtureClock {
    /// Return one fixed archive clock value for the disposable status owner.
    fn canonical_now(&mut self) -> Option<String> {
        Some("2026-11-24T00:00:00.000Z".to_owned())
    }
}

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one bounded process-owned archive root under the test scratch directory.
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "weather-native-edge-process-{}",
            std::process::id()
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("fixture archive root");
        Self(fs::canonicalize(root).expect("canonical fixture root"))
    }
}

impl Drop for FixtureRoot {
    /// Remove only this test's disposable archive root.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

struct PublicFixture {
    edge: Fixture,
    admin: FixtureAdminSession,
    json: FixtureWireJson,
}

struct SchedulerTarget {
    polling: Rc<Cell<bool>>,
    starts: Rc<Cell<usize>>,
    ticks: Rc<Cell<usize>>,
}

impl ForecastSchedulerPollTarget for SchedulerTarget {
    type Tick = ();
    type Error = Infallible;

    /// Admit exactly one polling owner after the public listener is ready.
    fn start_polling(&mut self) -> bool {
        self.starts.set(self.starts.get() + 1);
        !self.polling.replace(true)
    }

    /// Observe the actual deferred callback dispatched by the process's timer lane.
    fn run_due(&mut self) -> Result<Self::Tick, Self::Error> {
        self.ticks.set(self.ticks.get() + 1);
        Ok(())
    }

    /// Report whether this process currently owns polling admission.
    fn is_polling(&self) -> bool {
        self.polling.get()
    }

    /// Release the process-local polling admission with the process owner.
    fn stop_polling(&mut self) {
        self.polling.set(false);
    }
}

struct ProcessResources<RevisionArchive> {
    revision_archive: Option<RevisionArchive>,
    public_port: NonZeroU16,
    private_port: NonZeroU16,
    scheduler_polling: Rc<Cell<bool>>,
    scheduler_starts: Rc<Cell<usize>>,
    scheduler_ticks: Rc<Cell<usize>>,
}

impl<RevisionArchive: 'static> NativeEdgeStartupResources for ProcessResources<RevisionArchive> {
    type Site = ();
    type Admin = PublicFixture;
    type Evidence = CapturedForecastRequests;
    type RevisionArchive = RevisionArchive;
    type Archive = ();
    type Scheduler = NativeForecastSchedulerRuntime<SchedulerTarget>;
    type Assets = ();
    type Error = Infallible;

    /// Keep optional map metadata absent in this focused process fixture.
    fn load_site(&mut self) -> Option<Self::Site> {
        None
    }

    /// Initialize the actual public dispatch fixture after optional site loading.
    fn initialize_admin(&mut self, site: Option<&Self::Site>) -> Result<Self::Admin, Self::Error> {
        assert!(site.is_none());
        Ok(PublicFixture {
            edge: Fixture::new(),
            admin: FixtureAdminSession,
            json: FixtureWireJson::default(),
        })
    }

    /// Initialize the real forecast evidence owner used by edge dispatch.
    fn initialize_evidence(&mut self) -> Self::Evidence {
        CapturedForecastRequests::default()
    }

    /// Transfer the already bound native private archive owner into the process.
    fn initialize_revision_archive(&mut self) -> Result<Self::RevisionArchive, Self::Error> {
        Ok(self
            .revision_archive
            .take()
            .expect("single revision archive initialization"))
    }

    /// Keep unrelated online archive transfer disabled in this process fixture.
    fn initialize_archive(&mut self) -> Result<Self::Archive, Self::Error> {
        Ok(())
    }

    /// Retain the externally visible readiness counter in the scheduler owner.
    fn initialize_scheduler(
        &mut self,
        configuration: ForecastSchedulerConfiguration,
    ) -> Self::Scheduler {
        assert!(configuration.enabled);
        assert!(configuration.migration_ready);
        NativeForecastSchedulerRuntime::new(SchedulerTarget {
            polling: Rc::clone(&self.scheduler_polling),
            starts: Rc::clone(&self.scheduler_starts),
            ticks: Rc::clone(&self.scheduler_ticks),
        })
    }

    /// Supply the already embedded fixture assets.
    fn load_assets(&mut self) -> Result<Self::Assets, Self::Error> {
        Ok(())
    }

    /// Prove both requested sockets are bound before the one readiness transition.
    fn start_scheduler(&mut self, scheduler: &mut Self::Scheduler) -> Result<bool, Self::Error> {
        assert!(TcpListener::bind(("127.0.0.1", self.public_port.get())).is_err());
        assert!(TcpListener::bind(("127.0.0.1", self.private_port.get())).is_err());
        Ok(scheduler
            .start_on_public_ready()
            .expect("native scheduler timer installation"))
    }
}

/// Reserve two distinct nonzero loopback ports until both assignments are known.
fn reserve_loopback_ports() -> (NonZeroU16, NonZeroU16) {
    let public = TcpListener::bind(("127.0.0.1", 0)).expect("public port reservation");
    let private = TcpListener::bind(("127.0.0.1", 0)).expect("private port reservation");
    let public = NonZeroU16::new(public.local_addr().unwrap().port()).unwrap();
    let private = NonZeroU16::new(private.local_addr().unwrap().port()).unwrap();
    (public, private)
}

/// Exchange one close-delimited request with an already bound process listener.
fn exchange(address: SocketAddr, request: &[u8]) -> Vec<u8> {
    let mut socket = TcpStream::connect(address).expect("bound edge listener");
    socket
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    socket.write_all(request).unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).unwrap();
    response
}

/// Supply the bounded listener limits shared by both process sockets.
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

/// Prove one native edge process serves, reports, and stops both actual HTTP adapters.
#[test]
fn native_edge_process_serves_and_stops_public_and_private_listeners() {
    let fixture = FixtureRoot::new();
    let archive =
        bind_native_private_archive_stores(fixture.0.clone(), NodePrimitives, FixtureClock)
            .unwrap();
    let (public_port, private_port) = reserve_loopback_ports();
    let scheduler_polling = Rc::new(Cell::new(false));
    let scheduler_starts = Rc::new(Cell::new(0));
    let scheduler_ticks = Rc::new(Cell::new(0));
    let resources = ProcessResources {
        revision_archive: Some(archive),
        public_port,
        private_port,
        scheduler_polling: Rc::clone(&scheduler_polling),
        scheduler_starts: Rc::clone(&scheduler_starts),
        scheduler_ticks: Rc::clone(&scheduler_ticks),
    };
    let ports = NativeEdgeStartupPorts::loopback(resources, public_port, Some(private_port));
    let process = NativeEdgePreparedProcess::initialize(
        ports,
        EdgeStartupLifecycleConfiguration {
            archive_transfer_enabled: false,
            private_listener_enabled: true,
            scheduler: ForecastSchedulerConfiguration {
                enabled: true,
                migration_ready: true,
            },
        },
        listener_configuration(),
        listener_configuration(),
    )
    .unwrap();
    let public_address = process.public_local_addr().expect("public address");
    let private_address = process.private_local_addr().expect("private address");
    let runtime = NativeBackendRuntime::new().unwrap();
    let reports = runtime.block_on(async move {
        let (stop, stopped) = oneshot::channel();
        // exercise both actual handlers before resolving their shared shutdown
        let client = std::thread::spawn(move || {
            let public_response = exchange(
                public_address,
                b"GET /api/v1/sites/ballydidean/current HTTP/1.1\r\nHost: edge\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            );
            assert!(
                public_response.starts_with(b"HTTP/1.1 200 OK\r\n"),
                "{}",
                String::from_utf8_lossy(&public_response)
            );
            assert!(
                public_response
                    .windows(b"{\"path\":\"/api/v1/sites/ballydidean/current\"}".len())
                    .any(|window| {
                        window == b"{\"path\":\"/api/v1/sites/ballydidean/current\"}"
                    })
            );
            let body = format!(
                "{{\"logicalHourAt\":\"2026-10-08T01:00:00.000Z\",\"logicalKeySha256\":\"{}\"}}",
                "a".repeat(64)
            );
            let private_request = format!(
                "POST {PRIVATE_STATUS_PATH} HTTP/1.1\r\nHost: edge-private\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let private_response = exchange(private_address, private_request.as_bytes());
            assert!(
                private_response.starts_with(b"HTTP/1.1 200 OK\r\n"),
                "{}",
                String::from_utf8_lossy(&private_response)
            );
            assert!(
                private_response
                    .windows(b"\"state\":\"absent\"".len())
                    .any(|window| window == b"\"state\":\"absent\"")
            );
            let _ = stop.send(());
        });
        let reports = process
            .serve_with_private(
                // route public traffic through the actual decoded edge request and response owners
                |incoming, peer, scope, app: Rc<RefCell<_>>| async move {
                    let input = NativeEdgeHttpRequest::read(incoming, peer, &NativeWirePrimitives)
                        .expect("ordinary parsed edge request");
                    let reply = {
                        let mut app = app.borrow_mut();
                        app.inspect_scheduler(|scheduler| {
                            assert!(scheduler.target().is_polling());
                            assert_eq!(scheduler.target().starts.get(), 1);
                            // the process polls the deferred due callback after startup, before routing traffic
                            assert_eq!(scheduler.target().ticks.get(), 1);
                        });
                        let NativeEdgePublicApplication {
                            admin,
                            evidence,
                            ..
                        } = &mut *app;
                        let PublicFixture { edge, admin, json } = admin;
                        input
                            .dispatch(
                                RELEASE,
                                NativeEdgeRequestAdmissions::default(),
                                edge.ports(admin),
                                EdgeForecastCaptureAdmission {
                                    channel_bound_capture_active: false,
                                    scheduled_capture: None,
                                },
                                evidence,
                                json,
                            )
                            .expect("ordinary edge fixture response")
                    };
                    native_edge_http_response(reply, scope, |_observer, _terminal| {
                        panic!("current conditions must not install a forecast observer")
                    })
                },
                // route private traffic through the real archive status handler and native store
                |incoming, _peer, app: Rc<RefCell<_>>| async move {
                    let input =
                        NativePrivateArchiveHttpRequest::read(incoming, Duration::from_secs(4))
                            .await;
                    input.serve(
                        &mut app.borrow_mut().revision_archive,
                        &ArchiveWireCodec::new(NodePrimitives),
                        &mut ArchiveWireCodec::new(NodePrimitives),
                    )
                },
                async {
                    let _ = stopped.await;
                },
            )
            .await
            .unwrap();
        client.join().unwrap();
        reports
    });
    assert_eq!(scheduler_starts.get(), 1);
    assert_eq!(scheduler_ticks.get(), 1);
    assert!(!scheduler_polling.get());
    assert_eq!(reports.public.accepted_connections, 1);
    assert_eq!(reports.public.clean_connections, 1);
    assert_eq!(reports.public.protocol_errors, 0);
    let private_report = reports.private.expect("private listener report");
    assert_eq!(private_report.accepted_connections, 1);
    assert_eq!(private_report.clean_connections, 1);
    assert_eq!(private_report.protocol_errors, 0);
    assert!(TcpStream::connect_timeout(&public_address, Duration::from_millis(250)).is_err());
    assert!(TcpStream::connect_timeout(&private_address, Duration::from_millis(250)).is_err());
}
