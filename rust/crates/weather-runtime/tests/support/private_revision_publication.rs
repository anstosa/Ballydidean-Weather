//! Actual Node/Rust raw revision staging and publication differential.

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

const RETAINED_DIRECTORIES: [&str; 5] = [
    "revision-projections",
    "revision-stage-receipts",
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-publish-receipts",
];

/// Exercise ordinary and grouped raw stage/publish requests, including exact retries.
#[test]
fn private_revision_publication_matches_original_node() {
    let fixture = FixtureRoot::new("revision-publication");
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/private-revision-publication-node-oracle.mjs");
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
    let cases = fs::read_to_string(fixture.0.join("publication-cases.tsv")).unwrap();
    assert_root_cases(&fixture, &cases, "single");
    assert_root_cases(&fixture, &cases, "grouped");
}

/// Replay one isolated Node root through the actual raw Rust listener and native stores.
fn assert_root_cases(fixture: &FixtureRoot, cases: &str, root_name: &str) {
    let root = fixture.0.join(format!("rust-{root_name}"));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let mut stores =
        bind_native_private_archive_stores(root.clone(), NodePrimitives, Clock).unwrap();
    let decoder = ArchiveWireCodec::new(NodePrimitives);
    let mut encoder = ArchiveWireCodec::new(NodePrimitives);
    // replay every first-write and retry request for this isolated frontier
    for line in cases.lines() {
        let mut fields = line.split('\t');
        let case_root = fields.next().unwrap();
        let name = fields.next().unwrap();
        let target = fields.next().unwrap();
        assert!(fields.next().is_none(), "{name}");
        // leave the other isolated frontier to its own native store
        if case_root != root_name {
            continue;
        }
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
        let value = NodePrimitives.decode_json(&expected).unwrap();
        assert_eq!(
            response.body.as_bytes(),
            NodePrimitives.encode_sorted_json(&value).unwrap(),
            "{name}"
        );
    }
    let node_root = fixture.0.join(format!("node-{root_name}"));
    // compare exact projection, stage, commit/group, successor, and publication files
    for directory in RETAINED_DIRECTORIES {
        assert_directory_equal(&node_root, &root, directory);
    }
}

/// Compare every retained file name and byte without trusting response parity alone.
fn assert_directory_equal(node_root: &Path, rust_root: &Path, directory: &str) {
    let node = node_root.join(directory);
    let rust = rust_root.join(directory);
    let node_names = sorted_file_names(&node);
    let rust_names = sorted_file_names(&rust);
    assert_eq!(node_names, rust_names, "{directory} names");
    // compare every immutable retained document at its exact identity-derived name
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
