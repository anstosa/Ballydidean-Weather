#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use weather_edge::{NativeRevisionGapAckDecoder, REVISION_GAP_ACK_CONTRACT_VERSION};
use weather_runtime::NativeRevisionGapCodec;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Allocate one bounded disposable spool with the existing oracle's admitted basename.
    fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).expect("test temp root");
        let nonce = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test time")
            .as_nanos();
        let root = base.join(format!(
            "weather-native-spool-{}-{timestamp}-{nonce}",
            std::process::id()
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .expect("fixture root");
        Self(root)
    }

    /// Borrow the fixture-owned evidence root supplied to both command owners.
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureRoot {
    /// Remove only this test's bounded disposable evidence spool.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

struct NodeWitness {
    fields: BTreeMap<String, String>,
}

impl NodeWitness {
    /// Build one real source spool and retain the exact Node command witnesses.
    fn seed(root: &Path, mode: &str) -> Self {
        let oracle = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../weather-edge/tests/revision-gap-transfer-node-oracle.mjs");
        let output = Command::new("node")
            .arg(oracle)
            .arg("--native-fixture")
            .arg(root)
            .arg(mode)
            .output()
            .expect("revision gap fixture oracle");
        assert_success(&output, "revision gap fixture oracle");
        let fields = String::from_utf8(output.stdout)
            .expect("oracle UTF-8")
            .lines()
            .map(|line| {
                let (key, value) = line.split_once('=').expect("named oracle value");
                (key.to_owned(), value.to_owned())
            })
            .collect();
        Self { fields }
    }

    /// Borrow one scalar identity emitted by the independent Node fixture.
    fn field(&self, key: &str) -> &str {
        &self.fields[key]
    }

    /// Restore the exact trailing LF removed by the fixture's line protocol.
    fn bytes(&self, key: &str) -> Vec<u8> {
        format!("{}\n", self.field(key)).into_bytes()
    }
}

/// Run the real native evidence executable against one explicit disposable spool.
fn run_native(root: &Path, action: &str, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_weather-evidence"))
        .arg(action)
        .args(arguments)
        .env("WEATHER_ADJUSTMENT_EVIDENCE_ROOT", root)
        .output()
        .expect("native weather-evidence command")
}

/// Require one subprocess success while retaining its diagnostic on failure.
fn assert_success(output: &Output, command: &str) {
    assert!(
        output.status.success(),
        "{command}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Read every ordinary file in one fixed spool directory for retention checks.
fn directory_files(path: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            assert!(fs::metadata(&path).unwrap().is_file());
            (
                path.file_name().unwrap().to_str().unwrap().to_owned(),
                fs::read(path).unwrap(),
            )
        })
        .collect()
}

/// Compare the clock-independent typed acknowledgement fields with the Node receipt.
fn assert_acknowledgement_invariants(actual_bytes: &[u8], witness: &NodeWitness) {
    let codec = NativeRevisionGapCodec;
    let actual = codec
        .admitted_acknowledgement(actual_bytes)
        .expect("native acknowledgement");
    let expected = codec
        .admitted_acknowledgement(&witness.bytes("ack_bytes"))
        .expect("Node acknowledgement");
    assert_eq!(actual.contract_version, REVISION_GAP_ACK_CONTRACT_VERSION);
    assert_eq!(actual.contract_version, expected.contract_version);
    assert_eq!(actual.graph_manifest_sha256, expected.graph_manifest_sha256);
    assert_eq!(actual.member_root_sha256, expected.member_root_sha256);
    assert_eq!(actual.page_count, expected.page_count);
    assert_eq!(actual.page_sha256, expected.page_sha256);
    assert_eq!(
        actual.predecessor_frontier_sha256,
        expected.predecessor_frontier_sha256
    );
}

/// Exercise start, frozen page replay, acknowledgement replay and exact source retirement.
#[test]
fn native_gap_classified_flow_matches_node_and_retires_only_staged_members() {
    let fixture = FixtureRoot::new();
    let witness = NodeWitness::seed(fixture.path(), "classified");
    let transfer = fixture.path().join("revision-gap-transfer");
    let projection_root = fixture.path().join("revision-projections");
    let receipt_root = fixture.path().join("revision-stage-receipts");
    let gap_root = fixture.path().join("revision-gaps");
    let retained_gaps = directory_files(&gap_root);
    assert_eq!(directory_files(&projection_root).len(), 1);
    assert_eq!(directory_files(&receipt_root).len(), 1);
    assert_eq!(retained_gaps.len(), 1);

    let start = run_native(fixture.path(), "revision-gap-start-v1", &[]);
    assert_success(&start, "native revision-gap-start-v1");
    assert_eq!(start.stdout, witness.bytes("start_bytes"));

    let page_arguments = [witness.field("start"), witness.field("frontier")];
    // require both first publication and frozen retry to preserve exact Node page bytes
    for attempt in 0..2 {
        let page = run_native(fixture.path(), "revision-gap-page-v1", &page_arguments);
        assert_success(
            &page,
            &format!("native revision-gap-page-v1 attempt {attempt}"),
        );
        assert_eq!(page.stdout, witness.bytes("page_bytes"));
        assert_eq!(
            fs::read(transfer.join("pending.json")).unwrap(),
            witness.bytes("page_bytes")
        );
    }

    let ack_arguments = [witness.field("page"), witness.field("ack_graph")];
    let acknowledgement = run_native(fixture.path(), "revision-gap-ack-v1", &ack_arguments);
    assert_success(&acknowledgement, "native revision-gap-ack-v1");
    assert_acknowledgement_invariants(&acknowledgement.stdout, &witness);
    assert_eq!(
        fs::read(transfer.join("current.json")).unwrap(),
        acknowledgement.stdout
    );
    assert!(!transfer.join("pending.json").exists());
    assert!(directory_files(&projection_root).is_empty());
    assert!(directory_files(&receipt_root).is_empty());
    assert_eq!(directory_files(&gap_root), retained_gaps);

    let retry = run_native(fixture.path(), "revision-gap-ack-v1", &ack_arguments);
    assert_success(&retry, "native revision-gap-ack-v1 retry");
    assert_eq!(retry.stdout, acknowledgement.stdout);
    assert_eq!(
        fs::read(transfer.join("current.json")).unwrap(),
        acknowledgement.stdout
    );
    assert_eq!(directory_files(&gap_root), retained_gaps);
}

/// Preserve an ordinary in-flight stage while returning the exact idle Node page twice.
#[test]
fn native_gap_idle_page_matches_node_without_publishing_pending_state() {
    let fixture = FixtureRoot::new();
    let witness = NodeWitness::seed(fixture.path(), "idle");
    let transfer = fixture.path().join("revision-gap-transfer");
    let projection_root = fixture.path().join("revision-projections");
    let receipt_root = fixture.path().join("revision-stage-receipts");
    let retained_projections = directory_files(&projection_root);
    let retained_receipts = directory_files(&receipt_root);
    assert_eq!(retained_projections.len(), 1);
    assert_eq!(retained_receipts.len(), 1);

    let start = run_native(fixture.path(), "revision-gap-start-v1", &[]);
    assert_success(&start, "native idle revision-gap-start-v1");
    assert_eq!(start.stdout, witness.bytes("start_bytes"));

    let page_arguments = [witness.field("start"), witness.field("frontier")];
    // repeat collection to prove the idle source remains in flight without a cursor write
    for attempt in 0..2 {
        let page = run_native(fixture.path(), "revision-gap-page-v1", &page_arguments);
        assert_success(
            &page,
            &format!("native idle revision-gap-page-v1 attempt {attempt}"),
        );
        assert_eq!(page.stdout, witness.bytes("page_bytes"));
        assert!(!transfer.join("pending.json").exists());
    }

    assert_eq!(directory_files(&projection_root), retained_projections);
    assert_eq!(directory_files(&receipt_root), retained_receipts);
}
