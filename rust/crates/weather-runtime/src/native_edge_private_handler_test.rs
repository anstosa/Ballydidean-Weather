//! Positive loopback smoke for the concrete private archive Hyper handler.

use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener as StdTcpListener, TcpStream},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::Command,
    thread::{self, JoinHandle},
    time::Duration,
};

use hyper::{Request, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use weather_edge::{PrivateArchiveStorage, RevisionArchiveCanonicalClock};

use crate::{
    NativeEdgePrivateHandler, NativeModelEvidenceValues, bind_native_private_archive_stores,
};

const RETAINED_DIRECTORIES: [&str; 5] = [
    "revision-projections",
    "revision-stage-receipts",
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-publish-receipts",
];
const ACK_TIME: &str = "2026-10-08T03:00:00.000Z";

/// Deterministic source fixture clock used by the genuine native archive stores.
#[derive(Clone, Copy)]
struct FixtureClock;

impl RevisionArchiveCanonicalClock for FixtureClock {
    /// Return the same admitted archive time used by the predecessor fixture producer.
    fn canonical_now(&mut self) -> Option<String> {
        Some(ACK_TIME.to_owned())
    }
}

/// One bounded disposable root removed after the positive archive exchange.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one private fixture root without consulting production archive configuration.
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "weather-native-edge-private-handler-{}",
            std::process::id()
        ));
        let _prior = fs::remove_dir_all(&root);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("create private archive fixture root");
        Self(root.canonicalize().expect("admit fixture root"))
    }
}

impl Drop for FixtureRoot {
    /// Remove only the root allocated by this finite fixture.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

/// Serve a fixed number of close-delimited requests through the actual Hyper handler.
fn serve_loopback<Storage>(
    handler: NativeEdgePrivateHandler<Storage>,
    request_count: usize,
) -> (SocketAddr, JoinHandle<()>)
where
    Storage: PrivateArchiveStorage + Send + 'static,
{
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind private archive loopback");
    listener
        .set_nonblocking(true)
        .expect("set private archive loopback nonblocking");
    let address = listener.local_addr().expect("read loopback address");
    let server = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build private archive loopback runtime");
        runtime.block_on(async move {
            let listener = TcpListener::from_std(listener).expect("admit loopback listener");
            // retain one connection per close-delimited fixture request
            for _ in 0..request_count {
                let (socket, _) = listener.accept().await.expect("accept archive request");
                let request_handler = handler.clone();
                let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                    let request_handler = request_handler.clone();
                    async move { request_handler.serve(request).await }
                });
                http1::Builder::new()
                    .keep_alive(false)
                    .serve_connection(TokioIo::new(socket), service)
                    .await
                    .expect("serve private archive request");
            }
            handler.drain().await;
        });
    });
    (address, server)
}

/// Exchange one exact JSON request and return the complete HTTP response bytes.
fn exchange(address: SocketAddr, target: &str, body: &[u8]) -> Vec<u8> {
    let mut socket = TcpStream::connect(address).expect("connect private archive loopback");
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("bound private archive response wait");
    write!(
        socket,
        "POST {target} HTTP/1.1\r\nHost: archive\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write private archive request head");
    socket
        .write_all(body)
        .expect("write private archive request body");
    let mut response = Vec::new();
    socket
        .read_to_end(&mut response)
        .expect("read private archive response");
    response
}

/// Assert one successful JSON response against the predecessor fixture receipt.
fn assert_success(response: &[u8], expected_body: &[u8]) {
    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("complete private archive response head");
    let (head, body) = response.split_at(separator + 4);
    assert!(head.starts_with(b"HTTP/1.1 200 OK\r\n"));
    assert!(contains_ascii_case_insensitive(
        head,
        b"content-type: application/json; charset=utf-8\r\n"
    ));
    assert!(contains_ascii_case_insensitive(
        head,
        b"cache-control: no-store\r\n"
    ));
    assert_eq!(body, expected_body);
}

/// Find one ASCII header line without depending on Hyper's emitted field casing.
fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Return sorted flat file names for one immutable spool directory.
fn sorted_names(directory: &Path) -> Vec<OsString> {
    let mut names = fs::read_dir(directory)
        .expect("read retained directory")
        .map(|entry| entry.expect("read retained entry").file_name())
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Compare every immutable file written by the actual loopback handler with Node.
fn assert_retained_files(node_root: &Path, native_root: &Path) {
    // compare projection, stage, database, frontier, and publication evidence
    for directory in RETAINED_DIRECTORIES {
        let node_directory = node_root.join(directory);
        let native_directory = native_root.join(directory);
        let names = sorted_names(&node_directory);
        assert_eq!(sorted_names(&native_directory), names, "{directory} names");
        // compare every retained document without trusting only HTTP receipts
        for name in names {
            assert_eq!(
                fs::read(node_directory.join(&name)).expect("read Node retained document"),
                fs::read(native_directory.join(&name)).expect("read native retained document"),
                "{directory}/{}",
                name.to_string_lossy()
            );
        }
    }
}

/// Stage, reread, and publish one source-valid revision over the actual loopback listener.
#[test]
fn private_edge_handler_stages_reads_and_publishes_native_revision() {
    let fixture = FixtureRoot::new();
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/private-revision-publication-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .arg(&fixture.0)
        .output()
        .expect("run original archive fixture producer");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let native_root = fixture.0.join("native");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&native_root)
        .expect("create native archive root");
    let stores = bind_native_private_archive_stores(
        native_root.clone(),
        NativeModelEvidenceValues::new(),
        FixtureClock,
    )
    .expect("bind actual private archive stores");
    let handler = NativeEdgePrivateHandler::new(stores, Duration::from_secs(4));
    let (address, server) = serve_loopback(handler, 3);
    let cases = [
        (
            "single-stage-first",
            "/internal/adjustment-maintenance/revision/stage",
        ),
        (
            "single-stage-retry",
            "/internal/adjustment-maintenance/revision/stage",
        ),
        (
            "single-publish-first",
            "/internal/adjustment-maintenance/revision/publish",
        ),
    ];
    // drive first write, retained stage read, and publication through separate real sockets
    for (name, target) in cases {
        let request = fs::read(fixture.0.join(format!("{name}.request.json")))
            .expect("read source-valid archive request");
        let expected = fs::read(fixture.0.join(format!("{name}.response.json")))
            .expect("read original archive response");
        assert_success(&exchange(address, target, &request), &expected);
    }
    server.join().expect("join private archive loopback");
    assert_retained_files(&fixture.0.join("node-single"), &native_root);
}
