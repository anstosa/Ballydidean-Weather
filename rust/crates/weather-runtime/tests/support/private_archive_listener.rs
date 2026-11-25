//! Actual Node/Rust private listener and durable gap-store differential.

use super::{Clock, FixtureRoot, NodePrimitives};
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::process::Command;
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, PrivateArchiveTransportRequest,
    PrivateArchiveWireBody, serve_private_archive_wire_request,
};
use weather_runtime::bind_native_private_archive_stores;

/// Exercise concrete HTTP body/response mappings and all five native-store composition.
#[test]
fn private_listener_gap_routes_match_original_node() {
    let fixture = FixtureRoot::new("archive-listener");
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/private-archive-listener-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .arg(&fixture.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let root = fixture.0.join("rust");
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let mut stores =
        bind_native_private_archive_stores(root.clone(), NodePrimitives, Clock).unwrap();
    let decoder = ArchiveWireCodec::new(NodePrimitives);
    let mut encoder = ArchiveWireCodec::new(NodePrimitives);
    let cases = fs::read_to_string(fixture.0.join("listener-cases.tsv")).unwrap();
    // drive actual listener dispatch including first-write and ordinary retained retry cases
    for line in cases.lines() {
        let (name, target) = line.split_once('\t').unwrap();
        let request = fs::read(fixture.0.join(format!("{name}.request.json"))).unwrap();
        let response = serve_private_archive_wire_request(
            PrivateArchiveTransportRequest {
                method: "POST",
                target,
                content_type: Some("application/json"),
            },
            &mut PrivateArchiveWireBody::new(&request, &decoder),
            &mut stores,
            &mut encoder,
        );
        assert_eq!(response.status_code, 200, "{name}");
        assert_eq!(
            response.headers.content_length,
            response.body.as_bytes().len()
        );
        assert_eq!(response.headers.cache_control, "no-store");
        let expected = fs::read(fixture.0.join(format!("{name}.response.json"))).unwrap();
        let value = NodePrimitives.decode_json(&expected).unwrap();
        // node's first and retry responses may order keys differently; identities and values are exact
        assert_eq!(
            response.body.as_bytes(),
            NodePrimitives.encode_sorted_json(&value).unwrap(),
            "{name}"
        );
    }
    // verify byte-identical retained dispositions independently of the wire result
    for directory in [
        "rain-fixed-gauge-target-gaps",
        "shadow-revision-gaps",
        "revision-gaps",
    ] {
        let node = fixture.0.join("node").join(directory);
        let rust = root.join(directory);
        let names = fs::read_dir(&node)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 1);
        assert_eq!(fs::read_dir(&rust).unwrap().count(), 1);
        assert_eq!(
            fs::read(node.join(&names[0])).unwrap(),
            fs::read(rust.join(&names[0])).unwrap(),
            "{directory}"
        );
    }
}

/// Carry actual private archive operations over the adopted HTTP listener and one reused socket.
#[test]
fn private_archive_native_http_transport_matches_node() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;
    use tokio::{net::TcpListener, runtime::Builder, sync::oneshot, task::LocalSet};
    use weather_runtime::{
        NativeHttpListenerConfiguration, NativePrivateArchiveHttpRequest, serve_native_http1,
    };

    let fixture = FixtureRoot::new("native-http-archive");
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let original = Command::new("node")
        .arg(tests.join("private-archive-listener-node-oracle.mjs"))
        .arg(&fixture.0)
        .output()
        .unwrap();
    assert!(
        original.status.success(),
        "{}",
        String::from_utf8_lossy(&original.stderr)
    );
    let root = fixture.0.join("rust");
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let stores = Rc::new(RefCell::new(
        bind_native_private_archive_stores(root.clone(), NodePrimitives, Clock).unwrap(),
    ));
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let local = LocalSet::new();
    let report = local.block_on(&runtime, async {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, stopped) = oneshot::channel();
        let fixture_path = fixture.0.clone();
        let client = std::thread::spawn(move || {
            let output = Command::new("node")
                .arg(tests.join("native-http-archive-client.mjs"))
                .arg(fixture_path)
                .arg(port.to_string())
                .output();
            let _ = stop.send(());
            output
        });
        let config = NativeHttpListenerConfiguration::new(
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
            config,
            move |request, _peer| {
                let stores = Rc::clone(&stores);
                async move {
                    // do not retain a store borrow through body collection
                    let input =
                        NativePrivateArchiveHttpRequest::read(request, Duration::from_secs(4))
                            .await;
                    input.serve(
                        &mut *stores.borrow_mut(),
                        &ArchiveWireCodec::new(NodePrimitives),
                        &mut ArchiveWireCodec::new(NodePrimitives),
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
    assert_eq!(report.accepted_connections, 1);
    assert_eq!(report.clean_connections, 1);
    assert_eq!(report.protocol_errors, 0);
    // compare the actual retained bytes written through HTTP with predecessor storage
    for directory in [
        "rain-fixed-gauge-target-gaps",
        "shadow-revision-gaps",
        "revision-gaps",
    ] {
        let node = fixture.0.join("node").join(directory);
        let rust = root.join(directory);
        let entries = fs::read_dir(&node)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(fs::read_dir(&rust).unwrap().count(), 1);
        assert_eq!(
            fs::read(entries[0].path()).unwrap(),
            fs::read(rust.join(entries[0].file_name())).unwrap(),
            "{directory}"
        );
    }
}
