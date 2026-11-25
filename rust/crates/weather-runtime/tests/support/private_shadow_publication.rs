//! Actual producer-backed Node/Rust raw shadow publication differential.

use super::{Clock, FixtureRoot, NodePrimitives};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, EdgeWireResponse, PRIVATE_ARCHIVE_PUBLISH_TARGET,
    PRIVATE_ARCHIVE_STAGE_TARGET, PrivateArchiveStorage, PrivateArchiveTransportRequest,
    PrivateArchiveWireBody, serve_private_archive_wire_request,
};
use weather_runtime::bind_native_private_archive_stores;

const RETAINED_DIRECTORIES: [&str; 5] = [
    "shadow-revision-stages",
    "shadow-revision-capsules",
    "revision-commit-receipts",
    "revision-frontier-successors",
    "shadow-revision-publish-receipts",
];

/// Exercise compact v1 and value-bound v2 publication through the raw native listener.
#[test]
fn private_shadow_publication_matches_original_node() {
    let fixture = FixtureRoot::new("shadow-publication");
    let oracle =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/shadow-producer-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .arg(&fixture.0)
        .arg("publication")
        .output()
        .expect("shadow publication Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = fs::read_to_string(fixture.0.join("shadow-publication-cases.tsv")).unwrap();
    // keep each database frontier isolated while replaying both publication grammars
    for line in manifest.lines() {
        let (name, version) = line.split_once('\t').expect("shadow publication case");
        assert!(matches!(version, "v1" | "v2"), "{name}");
        assert_publication_case(&fixture, name);
    }
}

/// Replay one producer fixture through stage, publish, rejected blind retry, and restage retry.
fn assert_publication_case(fixture: &FixtureRoot, name: &str) {
    let root = fixture.0.join(format!("rust-publication-{name}"));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .expect("shadow publication Rust root");
    let mut stores =
        bind_native_private_archive_stores(root.clone(), NodePrimitives, Clock).unwrap();
    let decoder = ArchiveWireCodec::new(NodePrimitives);
    let mut encoder = ArchiveWireCodec::new(NodePrimitives);
    let stage_request = fs::read(
        fixture
            .0
            .join(format!("publication-{name}-stage.request.json")),
    )
    .unwrap();
    let publish_request = fs::read(
        fixture
            .0
            .join(format!("publication-{name}-publish.request.json")),
    )
    .unwrap();
    let expected_stage = fs::read(
        fixture
            .0
            .join(format!("publication-{name}-stage.response.json")),
    )
    .unwrap();
    let expected_publish = fs::read(
        fixture
            .0
            .join(format!("publication-{name}-publish.response.json")),
    )
    .unwrap();

    let stage = serve(
        &stage_request,
        PRIVATE_ARCHIVE_STAGE_TARGET,
        &decoder,
        &mut stores,
        &mut encoder,
    );
    assert_success(&stage, &expected_stage, &format!("{name} stage"));
    let publication = serve(
        &publish_request,
        PRIVATE_ARCHIVE_PUBLISH_TARGET,
        &decoder,
        &mut stores,
        &mut encoder,
    );
    assert_success(
        &publication,
        &expected_publish,
        &format!("{name} publication"),
    );

    let blind_retry = serve(
        &publish_request,
        PRIVATE_ARCHIVE_PUBLISH_TARGET,
        &decoder,
        &mut stores,
        &mut encoder,
    );
    assert_eq!(blind_retry.status_code, 409, "{name} blind retry");
    assert_eq!(
        blind_retry.body.as_bytes(),
        b"{\"error\":\"revision_archive_rejected\"}\n",
        "{name} blind retry"
    );

    let restage = serve(
        &stage_request,
        PRIVATE_ARCHIVE_STAGE_TARGET,
        &decoder,
        &mut stores,
        &mut encoder,
    );
    assert_success(&restage, &expected_stage, &format!("{name} restage"));
    let retry = serve(
        &publish_request,
        PRIVATE_ARCHIVE_PUBLISH_TARGET,
        &decoder,
        &mut stores,
        &mut encoder,
    );
    assert_success(&retry, &expected_publish, &format!("{name} retry"));

    let node_root = fixture.0.join(format!("node-publication-{name}"));
    // compare retired stage absence and every retained publication document exactly
    for directory in RETAINED_DIRECTORIES {
        assert_directory_equal(&node_root, &root, directory);
    }
}

/// Dispatch one exact JSON request through the raw listener and actual native stores.
fn serve(
    request: &[u8],
    target: &str,
    decoder: &ArchiveWireCodec<NodePrimitives>,
    stores: &mut impl PrivateArchiveStorage,
    encoder: &mut ArchiveWireCodec<NodePrimitives>,
) -> EdgeWireResponse {
    serve_private_archive_wire_request(
        PrivateArchiveTransportRequest {
            method: "POST",
            target,
            content_type: Some("application/json"),
        },
        &mut PrivateArchiveWireBody::new(request, decoder),
        stores,
        encoder,
    )
}

/// Compare one native success body to the semantic Node response value.
fn assert_success(response: &EdgeWireResponse, expected: &[u8], label: &str) {
    assert_eq!(response.status_code, 200, "{label}");
    assert_eq!(
        response.headers.content_length,
        response.body.as_bytes().len(),
        "{label}"
    );
    assert_eq!(response.headers.cache_control, "no-store", "{label}");
    let value = NodePrimitives
        .decode_json(expected)
        .expect("Node publication response value");
    assert_eq!(
        response.body.as_bytes(),
        NodePrimitives
            .encode_sorted_json(&value)
            .expect("normalized Node publication response"),
        "{label}"
    );
}

/// Compare every retained file name and byte without trusting response parity alone.
fn assert_directory_equal(node_root: &Path, rust_root: &Path, directory: &str) {
    let node = node_root.join(directory);
    let rust = rust_root.join(directory);
    let node_names = sorted_file_names(&node);
    let rust_names = sorted_file_names(&rust);
    assert_eq!(node_names, rust_names, "{directory} names");
    // compare each immutable member at its exact identity-derived path
    for name in node_names {
        assert_eq!(
            fs::read(node.join(&name)).unwrap(),
            fs::read(rust.join(&name)).unwrap(),
            "{directory}/{}",
            name.to_string_lossy()
        );
    }
}

/// Return deterministic flat spool file names for exact directory comparison.
fn sorted_file_names(directory: &Path) -> Vec<OsString> {
    let mut names = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    names.sort();
    names
}
