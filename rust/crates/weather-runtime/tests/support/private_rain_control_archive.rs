//! Raw native rain-control state stage and abandonment parity with the original store.

use super::{Clock, FixtureRoot, NodePrimitives};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, PrivateArchiveTransportRequest,
    PrivateArchiveWireBody, serve_private_archive_wire_request,
};
use weather_runtime::bind_native_private_archive_stores;

/// Exercise both raw rain-control routes through the actual native store and first-receipt retry.
#[test]
fn rain_control_state_routes_match_original_node_store() {
    let fixture = FixtureRoot::new("rain-control-archive");
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/rain-control-state-archive-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .arg(&fixture.0)
        .output()
        .expect("rain control Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let root = fixture.0.join("rust-rain-control");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .expect("rain control Rust root");
    let mut stores =
        bind_native_private_archive_stores(root.clone(), NodePrimitives, Clock).unwrap();
    let decoder = ArchiveWireCodec::new(NodePrimitives);
    let mut encoder = ArchiveWireCodec::new(NodePrimitives);
    let manifest = fs::read_to_string(fixture.0.join("rain-control-cases.tsv")).unwrap();
    let mut normalized_responses = Vec::new();
    // replay first writes and retries in the exact original route order
    for line in manifest.lines() {
        let (name, target) = line.split_once('\t').expect("rain control case");
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
            response.body.as_bytes().len(),
            "{name}"
        );
        assert_eq!(response.headers.cache_control, "no-store", "{name}");
        let expected = fs::read(fixture.0.join(format!("{name}.response.json"))).unwrap();
        let value = NodePrimitives
            .decode_json(&expected)
            .expect("Node response value");
        let normalized = NodePrimitives
            .encode_sorted_json(&value)
            .expect("normalized Node response");
        assert_eq!(response.body.as_bytes(), normalized, "{name}");
        normalized_responses.push(response.body.as_bytes().to_vec());
    }
    assert_eq!(normalized_responses.len(), 4);
    assert_eq!(normalized_responses[0], normalized_responses[1]);
    assert_eq!(normalized_responses[2], normalized_responses[3]);

    let node_root = fixture.0.join("node-rain-control");
    // compare the producer state and both immutable first receipts byte for byte
    for directory in [
        "rain-control-states",
        "rain-control-state-stage-receipts",
        "rain-control-state-gaps",
    ] {
        let node_directory = node_root.join(directory);
        let rust_directory = root.join(directory);
        let node_name = only_file_name(&node_directory);
        let rust_name = only_file_name(&rust_directory);
        assert_eq!(rust_name, node_name, "{directory} address");
        assert_eq!(
            fs::read(node_directory.join(&node_name)).unwrap(),
            fs::read(rust_directory.join(&rust_name)).unwrap(),
            "{directory} bytes"
        );
    }
    assert_eq!(
        fs::read_dir(node_root.join("revision-projections"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        fs::read_dir(root.join("revision-projections"))
            .unwrap()
            .count(),
        0
    );
}

/// Require one exact retained fixture member and return its content-addressed name.
fn only_file_name(directory: &Path) -> OsString {
    let mut entries = fs::read_dir(directory).expect("retained directory");
    let name = entries
        .next()
        .expect("retained member")
        .expect("retained entry")
        .file_name();
    assert!(entries.next().is_none(), "extra retained member");
    name
}
