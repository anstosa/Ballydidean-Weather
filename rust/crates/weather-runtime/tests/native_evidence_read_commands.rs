#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MAINTENANCE_ANCHOR: &str = concat!(
    r#"{"actionSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","archiveCommitOrdinal":"0","burnSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","candidateArtifactRootSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","candidateReportSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","captureEpochWitnessSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","confirmationAccessSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","confirmationChunkCount":24,"contractVersion":"adjustment-maintenance-anchor/v3","controlSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","controlVersion":"17","ctfState":"finalized","custodyCheckpointSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","dueKey":["confirmation/rain/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],"family":"rain","finalizationProofSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","finalizedAt":"2026-10-08T02:02:00.000Z","frontierSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","fullGraphVerifiedAt":"2026-10-08T02:00:00.000Z","fullMemberRootSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","graphManifestSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","inputSealSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","lifecycleLedgerRootSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","pageSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","policyReportSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","predecessorAnchorSha256":null,"publishedAt":"2026-10-08T02:01:00.000Z","requiredInputRootSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","sequence":"0","sourceCommit":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","workstationJournalHeadSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
    "\n",
);

const DEVELOPMENT_ANCHOR: &str = concat!(
    r#"{"actionKind":"shadow","actionSha256":"0b0273d14a584c2c33faf20c891c88f0b61f2f070d447a7388e41a1ea7e6727a","archiveCommitOrdinal":"1","artifactSha256":"433b137f0a0410e0db868b62834ec1d8d9f5cecb88fb4ff69e0763f4790b9891","candidateGraphSha256":"a5cc6effd562c616ba0a88582235d898de45b39f2f60f173cac1f24ab524c0fe","candidateSha256":"36f1a556908dcfcb35f7194a7451c0f4567ec2e404b5c6bd4b8ab97e026ae379","captureEpochWitnessSha256":"988dcbb38926f0cf28a4301e87adea36e54a6ebf667797b7df2e9be1cc22d516","contractVersion":"adjustment-development-custody-anchor/v1","controlSha256":"2cf3e0fc7958130777735e10524455d0e3cb76dd8d93b87c5be16820e380464c","controlVersion":"17","custodyAcknowledgedAt":"2026-10-08T00:35:00.000Z","custodyAcknowledgementSha256":"8d2da7efdefde571d34fb36e3eedc97dbfe16da5a13ecc25907c6ec669227265","custodyCheckpointSha256":"a3e896974dfdc686e94bf35f6790e0720a304ffce78cc532d6f90259709fc0cc","developmentGraphSha256":"726ada513ae7e26ab7cd1d0dd463a93e3339d3db0621abe5b62037ae849b239d","dueKey":"monthly/temperature/2026-10","family":"temperature","frontierSha256":"e1cefd06d563f8261e3c654cc39b6748a9a8bd2f5d34931e404ac1a44c4140de","fullGraphVerifiedAt":"2026-10-08T00:36:00.000Z","inputHeadSha256":"4eb3a04265fad8458545fa11594f01d0921618bd5f63036d66bec0aed20bfca6","lifecycleLedgerRootSha256":"29ca0587d8aa21f987de32cd321b8fe96ef5470de9a3c98e3e5d22594b8dfb7e","memberRootSha256":"c97b2f165380b932139d3738c15b39061b1d1da82e9ee2823f4f895ecb7d184e","pageSha256":"499131af4aef1ceb09622dd8351b8294aca48056d5996879908efdab911144c2","policyReportSha256":"7cf77184913fe717b0c0ec97e349a846a7a851d829ccdc26976c4e9c250f9a37","predecessorAnchorSha256":null,"registrationSha256":"830cc061b246d2e327a83c4f3339cc6dc7c1f98b7aad72cef85b5eb44faef803","sequence":"0","sourceCommit":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","sourceSha256":"e49c378dfd83ad9cb3ea32de7576bfd3a02b5ca9fe6163a0c7a296b3bf6935d0","startMemberSha256":null,"startSha256":"1d7974a9b4c24b580cce7e33f87b617de7c32770935777c57945a4b76b925519"}"#,
    "\n",
);

const RAIN_ANCHOR: &str = concat!(
    r#"{"actionKind":"control_reference","actionSha256":"db5e1ef5093d7d7c96c119d4ce16686c6fb6271154182eb448abb715899e6a08","archiveCommitOrdinal":"1","captureEpochWitnessSha256":"fd55e9c6f60480bf4cd4b817ebe681f880e68a24d1e3819fd4e8e8edac72e563","contractVersion":"adjustment-rain-control-custody-anchor/v1","controlSha256":"7b9da5b6313eab13fcbe62899636859f54afc4dd4c2016faffbe24a474dd337d","controlStateSha256":"1af476d6dc365437d0a38c440e789496a0a05e1c913e598aed1d62e163d15c6f","controlVersion":"17","custodyAcknowledgedAt":"2026-10-08T00:35:00.000Z","custodyAcknowledgementSha256":"0ded6fb11cdd8fee905ecdbc2a8f127b0606a563488c054a88ea7c4064596235","custodyCheckpointSha256":"273dc006e94e3f51a84039c3ce1a5f8de870b064dad3e806e7b4e70f62257719","dueMonth":"2026-10","fencingToken":"1","frontierSha256":"c96ef22bb45d62a1981804df2186e07453cbb35b091a37ef7a1205d3a02d763a","fullGraphVerifiedAt":"2026-10-08T00:36:00.000Z","graphManifestSha256":"0bd692e3e6f485f426736dbb6a8f8f217b4a464fefb3744e1ac534ea9877b520","memberRootSha256":"2afd3be70f07b9cc40d56c3005bf7d8141a40c01bfcf376751d14ba1ed81ebd0","ordinalArtifactSha256":"afffe24eb8f0b621d466b118eaf0eedd8bb8840d415a24c5c46aa34e63a5315f","pageSha256":"39d82c82a5597d63c2119ceb82917a42f7e2a936b17f78f8cb022070a8bbd8fd","predecessorAnchorSha256":null,"sequence":"0","sourceCommit":"cccccccccccccccccccccccccccccccccccccccc","sourceMemberRootSha256":"d6cd027e70b012285605cec939f3f624209201c2dc51d04cb6dcf2280dbb853c","sourceReceiptRootSha256":"71bde656e42ca107c3e6c06086f372bf04db3c767d4ce61fa9dfb925aed4bbe2","startMemberSha256":null,"startSha256":"10917e54f9b225b657f235b7e655c0e3180190be103eb0d2bda3d6cda195f4b2","workstationJournalHeadSha256":"983da1d0075db16aff9eca284ef7aaf24fa4fe9e3634aa5f2ad84880c0a561ba"}"#,
    "\n",
);

const NODE_COMMAND: &str = r#"
const { runAdjustmentArchiveCommand } = await import(process.env.WEATHER_COMMAND_MODULE);
await runAdjustmentArchiveCommand(process.argv.slice(1), {
  root: process.env.WEATHER_ADJUSTMENT_EVIDENCE_ROOT,
});
"#;

#[derive(Debug, Eq, PartialEq)]
enum SnapshotEntry {
    Directory(u32),
    File(u32, Vec<u8>),
}

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Allocate one bounded disposable fixture root for the named executable test.
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "weather-native-evidence-read-{}-{name}",
            std::process::id()
        ));
        create_directory(&root);
        Self(fs::canonicalize(root).expect("canonical fixture root"))
    }
}

impl Drop for FixtureRoot {
    /// Remove only the bounded scratch tree owned by this executable test.
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

/// Create one ordinary current-user fixture directory with the source mode.
fn create_directory(path: &Path) {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .expect("fixture directory");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

/// Write one exact canonical fixture document with its source mode.
fn write_document(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// Seed one nonempty valid document for each read-only anchor command.
fn seed_anchor_documents(root: &Path) {
    let fixtures = [
        ("maintenance-anchors", MAINTENANCE_ANCHOR),
        ("development-custody-anchors", DEVELOPMENT_ANCHOR),
        ("rain-control-custody-anchors", RAIN_ANCHOR),
    ];
    // populate each independent fixed-slot namespace once
    for (directory, contents) in fixtures {
        let directory = root.join(directory);
        create_directory(&directory);
        write_document(&directory.join("current.json"), contents);
    }
}

/// Capture every fixture namespace entry, mode and file byte for mutation checks.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, SnapshotEntry> {
    let mut entries = BTreeMap::new();
    snapshot_path(root, root, &mut entries);
    entries
}

/// Walk only the bounded fixture tree and retain its complete ordinary contents.
fn snapshot_path(root: &Path, path: &Path, entries: &mut BTreeMap<PathBuf, SnapshotEntry>) {
    let metadata = fs::symlink_metadata(path).unwrap();
    let relative = path.strip_prefix(root).unwrap().to_owned();
    // descend only through the ordinary fixture directories
    if metadata.is_dir() {
        entries.insert(relative, SnapshotEntry::Directory(metadata.mode() & 0o7777));
        // retain every child before comparing post-command state
        for entry in fs::read_dir(path).unwrap() {
            snapshot_path(root, &entry.unwrap().path(), entries);
        }
    } else {
        assert!(metadata.is_file(), "fixture contains an unexpected entry");
        entries.insert(
            relative,
            SnapshotEntry::File(metadata.mode() & 0o7777, fs::read(path).unwrap()),
        );
    }
}

/// Run the original Node command against the selected disposable evidence root.
fn run_node_command(root: &Path, action: &str, arguments: &[&str]) -> Output {
    let module = fs::canonicalize(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../deploy/scripts/adjustment-evidence-store.mjs"),
    )
    .expect("original adjustment evidence command module");
    Command::new("node")
        .arg("--input-type=module")
        .arg("--eval")
        .arg(NODE_COMMAND)
        .arg("--")
        .arg(action)
        .args(arguments)
        .env("WEATHER_COMMAND_MODULE", module)
        .env("WEATHER_ADJUSTMENT_EVIDENCE_ROOT", root)
        .output()
        .expect("original Node command")
}

/// Run the real native executable against the selected disposable evidence root.
fn run_native_command(root: &Path, action: &str, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_weather-evidence"))
        .arg(action)
        .args(arguments)
        .env("WEATHER_ADJUSTMENT_EVIDENCE_ROOT", root)
        .output()
        .expect("native weather-evidence command")
}

/// Require an executable command to succeed and report its captured diagnostic on failure.
fn assert_success(output: &Output, command: &str) {
    assert!(
        output.status.success(),
        "{command}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Compare the three real nonempty native anchor readers with their Node predecessors.
#[test]
fn native_anchor_read_commands_match_node_without_writes() {
    let fixture = FixtureRoot::new("anchors");
    seed_anchor_documents(&fixture.0);
    let before = snapshot(&fixture.0);
    let actions = [
        "maintenance-anchor-status-v2",
        "development-custody-anchor-current-v1",
        "rain-control-custody-anchor-current-v1",
    ];

    // compare exact stdout for every ordinary nonempty read command
    for action in actions {
        let expected = run_node_command(&fixture.0, action, &[]);
        assert_success(&expected, &format!("Node {action}"));
        let actual = run_native_command(&fixture.0, action, &[]);
        assert_success(&actual, &format!("native {action}"));
        assert_eq!(actual.stdout, expected.stdout, "{action}");
    }

    assert_eq!(snapshot(&fixture.0), before);
}

/// Compare the real nonempty native cold-page reader with its original Node predecessor.
#[test]
fn native_revision_cold_page_matches_node_without_writes() {
    let fixture = FixtureRoot::new("cold-page");
    let oracle =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native-cold-custody-node-oracle.mjs");
    let setup = Command::new("node")
        .arg(oracle)
        .arg("fixtures")
        .arg(&fixture.0)
        .arg("mixed")
        .output()
        .expect("cold-page fixture setup");
    assert_success(&setup, "cold-page fixture setup");

    let root = fixture.0.join("rust");
    let operands = fs::read_to_string(fixture.0.join("operands.txt")).unwrap();
    let operands = operands.lines().take(6).collect::<Vec<_>>();
    assert_eq!(operands.len(), 6);
    let before = snapshot(&root);
    let expected = run_node_command(&root, "revision-cold-page-v1", &operands);
    assert_success(&expected, "Node revision-cold-page-v1");
    let actual = run_native_command(&root, "revision-cold-page-v1", &operands);
    assert_success(&actual, "native revision-cold-page-v1");

    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(
        actual.stdout,
        fs::read(fixture.0.join("page.json")).unwrap()
    );
    assert_eq!(snapshot(&root), before);
}
