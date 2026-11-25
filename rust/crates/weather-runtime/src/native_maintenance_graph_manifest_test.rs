use super::*;
use std::process::Command;

/// Compare graph ranges, sorting, cross links and identities with the actual source builder.
#[test]
fn native_graph_manifest_matches_source_bytes_and_reopens() {
    let mut entries = Vec::new();
    for (kind, identity, offset, length) in [
        ("z/data", "a", 10, 4),
        ("a/row", "b", 0, 10),
        ("a/row", "c", 14, 0),
    ] {
        entries.push(NativeMaintenanceGraphEntry {
            kind: kind.into(),
            identity_sha256: identity.repeat(64),
            object_sha256: "d".repeat(64),
            member_offset: offset,
            member_length: length,
            member_sha256: "e".repeat(64),
        });
    }
    let links = vec![NativeMaintenanceGraphCrossLink {
        from_identity_sha256: "b".repeat(64),
        relation: "contains".into(),
        to_identity_sha256: "a".repeat(64),
    }];
    let native = build_native_maintenance_graph_manifest(None, entries, links).unwrap();
    let expected=Command::new("node").args(["--input-type=module","-e",r#"
import {pathToFileURL} from 'node:url';
const {buildGraphManifest,canonicalJsonBytes}=await import(pathToFileURL(process.argv[2]));
const entries=[["z/data","a",10,4],["a/row","b",0,10],["a/row","c",14,0]].map(([kind,id,memberOffset,memberLength])=>({kind,identitySha256:id.repeat(64),objectSha256:"d".repeat(64),memberOffset,memberLength,memberSha256:"e".repeat(64)}));
const links=[{fromIdentitySha256:"b".repeat(64),relation:"contains",toIdentitySha256:"a".repeat(64)}];
process.stdout.write(canonicalJsonBytes(buildGraphManifest({predecessorGraphSha256:null,entries,crossLinks:links})));
"#,"graph-oracle"]).arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts/research/adjustment_plaintext_archive.mjs").canonicalize().unwrap()).output().unwrap();
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    let actual = native.canonical_bytes().unwrap();
    assert_eq!(actual, expected.stdout);
    assert_eq!(
        read_native_maintenance_graph_manifest(&actual),
        Some(native)
    );
}
