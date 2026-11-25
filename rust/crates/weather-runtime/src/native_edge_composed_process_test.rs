//! Concrete public-only edge composition and coordinated shutdown fixture.

use std::{
    fs,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    num::NonZeroU16,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use weather_edge::{
    EdgeStartupEnvironment, EdgeStartupLifecycleConfiguration, EdgeXweatherCredentialByteReader,
    NativeEdgeStartupAdmission, project_edge_startup_configuration_with_credentials,
};

use crate::{
    NativeBackendRuntime, NativeEdgeConfiguration, NativeEdgeOrigins, NativeEdgePreparedProcess,
    NativeEdgeRuntimeResources, NativeEdgeStartupPorts, NativeForecastAllocationStatfs,
    NativeForecastArchiveCodec, NativeHttpListenerConfiguration, NativeOutboundHttp,
    NativeScheduledCaptureRegistry, compose_native_edge_process,
};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-local evidence and administrator root removed after the test.
    fn new() -> Self {
        let id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "weather-native-edge-composed-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create edge composition fixture");
        Self(root)
    }
}

impl Drop for FixtureRoot {
    /// Remove every filesystem artifact created by the genuine storage owners.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct MissingCredentials;

impl EdgeXweatherCredentialByteReader for MissingCredentials {
    type Error = io::Error;

    /// Keep the public shell fixture independent from host credential files.
    fn read_optional(&mut self, _: &Path) -> Result<Option<Vec<u8>>, Self::Error> {
        Ok(None)
    }
}

/// Refuse any accidental forecast compression on this static-shell-only path.
fn unexpected_compression(_: &[u8]) -> Option<Vec<u8>> {
    panic!("static shell request unexpectedly invoked forecast compression")
}

/// Prove concrete resources compose and serve public-only before coordinated shutdown.
#[test]
fn concrete_public_only_process_serves_original_shell_and_drains() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("canonical repository root");
    let fixture = FixtureRoot::new();
    let (mut configuration, map_credentials) = project_edge_startup_configuration_with_credentials(
        EdgeStartupEnvironment::new(&repository),
        &mut NativeEdgeStartupAdmission::new(NativeEdgeOrigins),
        &mut MissingCredentials,
    )
    .expect("admit isolated edge configuration");
    configuration.adjustment_evidence_root = fixture.0.join("evidence");
    configuration.admin_auth_path = fixture.0.join("admin-auth.json");
    configuration.admin_bootstrap_token_path = fixture.0.join("bootstrap-token");
    configuration.property_sensor_layout_path = fixture.0.join("property-sensor-layout.json");
    configuration.adjustment_maintenance_internal_port = None;
    let lifecycle = EdgeStartupLifecycleConfiguration::from(&configuration);
    let startup = NativeEdgeConfiguration {
        configuration,
        map_credentials,
    };
    let codec =
        NativeForecastArchiveCodec::new(unexpected_compression as fn(&[u8]) -> Option<Vec<u8>>);
    let resources = NativeEdgeRuntimeResources::new(
        startup,
        Arc::new(NativeOutboundHttp::new().expect("start native HTTP client")),
        codec,
        NativeForecastAllocationStatfs::new(),
        NativeScheduledCaptureRegistry::default(),
    )
    .expect("bind concrete edge resources");
    let public_port = reserve_loopback_port();
    let listener = NativeHttpListenerConfiguration::new(
        4,
        Duration::from_secs(3),
        Duration::from_secs(10),
        Duration::from_secs(3),
        32 * 1024,
        false,
    )
    .expect("admit listener limits");
    let prepared = NativeEdgePreparedProcess::initialize(
        NativeEdgeStartupPorts::loopback(resources, public_port, None),
        lifecycle,
        listener,
        listener,
    )
    .expect("initialize concrete public-only process");
    let address = prepared.public_local_addr().expect("bound public address");
    assert!(prepared.private_local_addr().is_none());
    let runtime = NativeBackendRuntime::new().expect("start backend runtime");
    runtime.block_on(async move {
        // compose inside the running process reactor, as installed startup does
        let process = compose_native_edge_process(prepared, Duration::from_secs(3), Duration::ZERO)
            .expect("compose public-only process without a private body deadline");
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let client = thread::spawn(move || {
            let response = request(address, "/forecast").expect("read public shell response");
            let _ = stop.send(());
            response
        });
        let reports = process
            .serve(async move {
                let _ = stopped.await;
            })
            .await
            .expect("serve and drain concrete edge process");
        let response = String::from_utf8(client.join().expect("join shell client"))
            .expect("shell response is UTF-8");
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("/assets/development/"));
        assert!(reports.private.is_none());
    });
}

/// Reserve one nonzero loopback port for immediate transfer to the startup owner.
fn reserve_loopback_port() -> NonZeroU16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve loopback port");
    let port = listener.local_addr().expect("read loopback port").port();
    NonZeroU16::new(port).expect("operating system selected a nonzero port")
}

/// Issue one close-delimited ordinary shell request without contacting an upstream.
fn request(address: SocketAddr, path: &str) -> io::Result<Vec<u8>> {
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
