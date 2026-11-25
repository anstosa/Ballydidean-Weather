use super::*;
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    /// Create the existing oracle's bounded disposable spool root.
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "weather-native-spool-{}-314-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    /// Remove just the test's disposable source spool.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Compare the full native graph segment to the actual predecessor's genuine classified gap page.
#[test]
fn native_gap_segment_matches_predecessor_members_and_checkpoint() {
    let fixture = Fixture::new();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let output = Command::new("node")
        .arg(repo.join("rust/crates/weather-edge/tests/revision-gap-transfer-node-oracle.mjs"))
        .arg("--native-fixture")
        .arg(&fixture.0)
        .arg("classified")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let witness = String::from_utf8(output.stdout).unwrap();
    let field = |name: &str| {
        witness
            .lines()
            .find_map(|line| {
                line.split_once('=')
                    .filter(|(key, _)| *key == name)
                    .map(|(_, value)| value)
            })
            .unwrap()
    };
    let start = read_native_maintenance_gap_start(format!("{}\n", field("start_bytes")).as_bytes())
        .unwrap();
    let page = format!("{}\n", field("page_bytes"));
    let segment = build_native_maintenance_gap_segment(page.as_bytes()).unwrap();
    let expected=Command::new("node").args(["--input-type=module","-e",r#"
import {pathToFileURL} from "node:url";
const {buildAdjustmentRevisionGapGraphSegment}=await import(pathToFileURL(process.argv[2]));
const segment=buildAdjustmentRevisionGapGraphSegment(JSON.parse(process.argv[3]));
process.stdout.write(JSON.stringify({members:segment.members.map(m=>({kind:m.kind,identitySha256:m.identitySha256,payloadBase64:m.payload.toString("base64")})),crossLinks:segment.crossLinks}));
"#,"gap-oracle"]).arg(repo.join("deploy/scripts/adjustment-evidence-store.mjs")).arg(&page).output().unwrap();
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    let expected: serde_json::Value = serde_json::from_slice(&expected.stdout).unwrap();
    let members:Vec<_>=segment.members.iter().map(|m|serde_json::json!({"kind":m.kind,"identitySha256":format_digest(m.identity_sha256),"payloadBase64":NativeWirePrimitives.encode_standard_base64(&m.payload).unwrap()})).collect();
    let links:Vec<_>=segment.cross_links.iter().map(|l|serde_json::json!({"fromIdentitySha256":l.from_identity_sha256,"toIdentitySha256":l.to_identity_sha256,"relation":l.relation})).collect();
    assert_eq!(
        serde_json::json!({"members":members,"crossLinks":links}),
        expected
    );
    assert_eq!(
        start.start_sha256,
        crate::native_revision_gap_decode::decode_page(&NativeWirePrimitives, page.as_bytes())
            .unwrap()
            .start_sha256
    );
}
