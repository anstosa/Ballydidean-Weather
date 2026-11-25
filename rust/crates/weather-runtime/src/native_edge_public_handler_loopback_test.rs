//! Actual native listener/route/terminal-lane integration fixture.

use std::{
    io::{self, Read, Write},
    net::TcpStream,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

use weather_edge::{
    EdgeStartupEnvironment, EdgeXweatherCredentialByteReader, ForecastArchiveAppendOutcome,
    ForecastCaptureSource, ForecastCaptureVersion, ForecastEvidenceOutcome, ForecastEvidenceWindow,
    ForecastMapSite, ForecastResponseEvidenceStore, ForecastScheduledEvidenceArchive,
    NativeEdgeStartupAdmission, PropertySensorMapCenter,
    project_edge_startup_configuration_with_credentials,
};

use crate::{
    NativeBackendRuntime, NativeEdgeAdminAdmissionOwner, NativeEdgeAdminOwner,
    NativeEdgeConfiguration, NativeEdgeOrigins, NativeEdgePostResponseArchive,
    NativeEdgePostResponseLane, NativeEdgePublicHandler, NativeEdgePublicRouteGraph,
    NativeEdgeRouteExecution, NativeHttpListenerConfiguration, NativeOutboundHttp,
    NativeScheduledCaptureRegistry, NativeSharedForecastResponseEvidenceStore,
    initialize_native_edge_public_assets, serve_native_edge_http1,
};

struct FixtureCredentials;

impl EdgeXweatherCredentialByteReader for FixtureCredentials {
    type Error = io::Error;

    /// Keep the ordinary fixture separate from actual credential files.
    fn read_optional(&mut self, _: &Path) -> Result<Option<Vec<u8>>, Self::Error> {
        Ok(None)
    }
}

struct StaticFixtureEvidence;

impl ForecastResponseEvidenceStore for StaticFixtureEvidence {
    type Prepared = ();
    type Error = io::Error;

    /// No forecast evidence preparation belongs to the actual static and redirect routes.
    fn prepare(
        &mut self,
        _: ForecastCaptureSource<'_>,
        _: Option<ForecastEvidenceWindow>,
        _: ForecastCaptureVersion,
    ) -> Option<()> {
        panic!("static route unexpectedly prepares forecast evidence")
    }
    /// No terminal forecast write belongs to these routes.
    fn commit(&mut self, _: &(), _: &str) -> Result<ForecastEvidenceOutcome, io::Error> {
        panic!("static route unexpectedly commits forecast evidence")
    }
    /// No forecast observer can report close on these ordinary routes.
    fn note_close_without_finish(&mut self) {
        panic!("unexpected forecast observer");
    }
    /// No forecast observer can report a write failure on these ordinary routes.
    fn note_write_failure(&mut self) {
        panic!("unexpected forecast observer");
    }
}

struct FixtureArchive;

impl ForecastScheduledEvidenceArchive<()> for FixtureArchive {
    type Error = io::Error;

    /// Ordinary public shell requests cannot append scheduled evidence.
    fn append_committed(
        &mut self,
        _: &str,
        _: &(),
    ) -> Result<ForecastArchiveAppendOutcome, io::Error> {
        panic!("unexpected scheduled archive work")
    }
}

/// Exercise the real native socket, complete route graph, and handler, without any live upstream traffic.
#[test]
fn native_edge_public_handler_loopback_serves_real_shell_and_preview() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let (configuration, map_credentials) = project_edge_startup_configuration_with_credentials(
        EdgeStartupEnvironment::new(&root),
        &mut NativeEdgeStartupAdmission::new(NativeEdgeOrigins),
        &mut FixtureCredentials,
    )
    .unwrap();
    let mut startup = NativeEdgeConfiguration {
        configuration,
        map_credentials,
    };
    let fixture = std::env::temp_dir().join(format!(
        "weather-native-edge-handler-{}",
        std::process::id()
    ));
    let admin = NativeEdgeAdminOwner::new(
        fixture.join("auth.json"),
        fixture.join("settings.json"),
        fixture.join("layout.json"),
        PropertySensorMapCenter {
            latitude: 47.95043,
            longitude: -122.42797,
        },
    )
    .unwrap();
    let admin_admission = NativeEdgeAdminAdmissionOwner::from_configuration(&startup.configuration);
    let site = Some(ForecastMapSite {
        latitude: 47.95043,
        longitude: -122.42797,
    });
    let assets = initialize_native_edge_public_assets(&startup.configuration, site).unwrap();
    let graph = Arc::new(
        NativeEdgePublicRouteGraph::initialize(
            &mut startup,
            admin,
            assets,
            site,
            Arc::new(NativeOutboundHttp::new().unwrap()),
        )
        .unwrap(),
    );
    let routes = NativeEdgeRouteExecution::new(graph, NonZeroUsize::new(2).unwrap());
    let runtime = NativeBackendRuntime::new().unwrap();
    runtime.block_on(async move {
        let post_response = NativeEdgePostResponseLane::new(
            NativeSharedForecastResponseEvidenceStore::new(StaticFixtureEvidence),
            NativeEdgePostResponseArchive::<FixtureArchive>::Disabled,
        );
        let handler = NativeEdgePublicHandler::new(
            routes,
            admin_admission,
            post_response,
            NativeScheduledCaptureRegistry::default(),
            startup.configuration.release,
            Duration::from_secs(3),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
        let client = thread::spawn(move || {
            let shell = request(address, "/forecast");
            let preview = request(
                address,
                "/__rac/browser-device?mode=mobile&location=%2Fforecast%3Fdays%3D1",
            );
            let _ = stopped_tx.send(());
            (shell, preview)
        });
        let service = handler.clone();
        let config = NativeHttpListenerConfiguration::new(
            4,
            Duration::from_secs(3),
            Duration::from_secs(10),
            Duration::from_secs(3),
            32 * 1024,
            false,
        )
        .unwrap();
        let report = serve_native_edge_http1(
            listener,
            config,
            move |incoming, peer, scope| {
                let handler = service.clone();
                async move { handler.serve(incoming, peer, scope, false).await }
            },
            async move {
                let _ = stopped_rx.await;
            },
        )
        .await
        .unwrap();
        handler.close_and_drain().await.unwrap();
        let (shell, preview) = client.join().unwrap();
        let shell = String::from_utf8(shell.unwrap()).unwrap();
        assert!(shell.starts_with("HTTP/1.1 200"));
        assert!(shell.contains("/assets/development/"));
        let preview = String::from_utf8(preview.unwrap())
            .unwrap()
            .to_ascii_lowercase();
        assert!(preview.starts_with("http/1.1 302"));
        assert!(preview.contains("location: /forecast?days=1\r\n"));
        let _ = report;
    });
}

/// Issue one disposable ordinary request and retain its complete socket response.
fn request(address: std::net::SocketAddr, path: &str) -> io::Result<Vec<u8>> {
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(3)))?;
    write!(
        socket,
        "GET {path} HTTP/1.1\r\nHost: weather.invalid\r\nConnection: close\r\n\r\n"
    )?;
    let mut bytes = Vec::new();
    socket.read_to_end(&mut bytes)?;
    Ok(bytes)
}
