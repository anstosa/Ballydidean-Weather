use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use weather_runtime::{
    CustodyAnchorControl, NativeCustodyAnchorInstallFiles, NativeWirePrimitives,
    install_custody_anchor_command,
};

const SLOT_NAMES: [&str; 3] = ["current.json", "pending.json", "previous.json"];

#[derive(Default)]
struct Document {
    bytes: Vec<u8>,
}

struct Case {
    name: String,
    action: String,
    directory: String,
    control_sha: String,
    control_version: String,
    argument: String,
    input: String,
    witness: String,
    acknowledgements: [String; 2],
    missing_dir: String,
    present: bool,
    after_present: bool,
    before: [String; 3],
    expected: Option<String>,
    after: [String; 3],
}

#[derive(Default)]
struct Oracle {
    documents: BTreeMap<String, Document>,
    cases: Vec<Case>,
}

// retain the exact one-lf framing used by both original current commands
fn line(value: &str) -> Vec<u8> {
    format!("{value}\n").into_bytes()
}

impl Oracle {
    // invoke the actual original Node commands and retain only their closed fixture protocol
    fn load() -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/custody-anchor-install-node-oracle.mjs");
        let child = Command::new("node")
            .arg(script)
            .output()
            .expect("Node custody anchor install oracle");
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        let mut oracle = Self::default();
        // retain every original document and expected command outcome exactly once
        for row in stdout.lines() {
            match row.split('\t').collect::<Vec<_>>().as_slice() {
                ["doc", name, bytes] => {
                    oracle
                        .documents
                        .entry((*name).to_owned())
                        .or_default()
                        .bytes = line(bytes);
                }
                // the executable codec now derives these values from the actual document bytes
                ["node", _, _, _, _] | ["hash", _, _] | ["clock", _, _] => {}
                [
                    "case",
                    name,
                    action,
                    control_sha,
                    control_version,
                    argument,
                    input,
                    witness,
                    ack_current,
                    ack_previous,
                    before_root,
                    missing_dir,
                    current,
                    pending,
                    previous,
                    expected,
                    after_root,
                    after_current,
                    after_pending,
                    after_previous,
                ] => {
                    let (action, directory) = match *action {
                        "dev" => (
                            "development-custody-anchor-install-v1",
                            "development-custody-anchors",
                        ),
                        "rain" => (
                            "rain-control-custody-anchor-install-v1",
                            "rain-control-custody-anchors",
                        ),
                        _ => panic!("unexpected custody anchor action"),
                    };
                    oracle.cases.push(Case {
                        name: (*name).to_owned(),
                        action: action.to_owned(),
                        directory: directory.to_owned(),
                        control_sha: (*control_sha).to_owned(),
                        control_version: (*control_version).to_owned(),
                        argument: (*argument).to_owned(),
                        input: (*input).to_owned(),
                        witness: (*witness).to_owned(),
                        acknowledgements: [*ack_current, *ack_previous].map(str::to_owned),
                        missing_dir: (*missing_dir).to_owned(),
                        present: *before_root == "present",
                        after_present: *after_root == "present",
                        before: [*current, *pending, *previous].map(str::to_owned),
                        expected: (*expected != "error").then(|| (*expected).to_owned()),
                        after: [*after_current, *after_pending, *after_previous].map(str::to_owned),
                    });
                }
                _ => panic!("unexpected custody anchor oracle row: {row}"),
            }
        }
        oracle
    }
}

struct FixtureRoot(PathBuf);

impl Drop for FixtureRoot {
    // remove only this test's uniquely owned disposable fixture tree
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// write one fixture-owned exact private file
fn write_fixture(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

// project one exact expected slot map from the oracle's document identities
fn expected_files(names: &[String; 3], oracle: &Oracle) -> BTreeMap<String, Vec<u8>> {
    SLOT_NAMES
        .iter()
        .zip(names)
        .filter(|(_, name)| *name != "-")
        .map(|(filename, name)| ((*filename).to_owned(), oracle.documents[name].bytes.clone()))
        .collect()
}

// read the complete directory namespace and verify every retained file is private
fn read_files(directory: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let metadata = fs::metadata(&path).unwrap();
            assert!(metadata.is_file());
            assert_eq!(metadata.mode() & 0o777, 0o600);
            (
                path.file_name().unwrap().to_str().unwrap().to_owned(),
                fs::read(path).unwrap(),
            )
        })
        .collect()
}

const REQUIRED_DIRS: [&str; 13] = [
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-projections",
    "shadow-revision-capsules",
    "rain-control-states",
    "rain-control-state-stage-receipts",
    "revision-custody-acknowledgements",
    "revision-gaps",
    "revision-publish-receipts",
    "revision-spool-locks",
    "revision-stage-receipts",
    "shadow-revision-gaps",
    "shadow-revision-publish-receipts",
];

// create one actual current-user private fixture directory
fn private_directory(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

// preserve absence for independently selected optional owner identities
fn optional(value: &str) -> Option<&str> {
    (value != "-").then_some(value)
}

// materialize only the explicitly selected acknowledgement slots
fn install_acknowledgements(
    evidence: &Path,
    case: &Case,
    oracle: &Oracle,
) -> BTreeMap<String, Vec<u8>> {
    let mut expected = BTreeMap::new();
    // retain the source's exact fixed cold and custody directory population
    for name in REQUIRED_DIRS {
        if case.missing_dir != name {
            private_directory(&evidence.join(name));
        }
    }
    let directory = evidence.join("revision-custody-acknowledgements");
    // preserve every selected canonical acknowledgement byte for later readback
    for (name, document) in ["current.json", "previous.json"]
        .into_iter()
        .zip(&case.acknowledgements)
    {
        if document != "-" {
            let bytes = oracle.documents[document].bytes.clone();
            write_fixture(&directory.join(name), &bytes);
            expected.insert(name.to_owned(), bytes);
        }
    }
    expected
}

/// Compare original Node anchor installation with actual Rust native persistence and retained custody.
#[test]
fn custody_anchor_installers_match_node_transitions() {
    let oracle = Oracle::load();
    assert_eq!(oracle.cases.len(), 21);
    let root = std::env::temp_dir().join(format!(
        "weather-custody-anchor-install-parity-{}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("unique fixture root");
    let fixture = FixtureRoot(root);
    let directory = fs::canonicalize(&fixture.0).unwrap();
    let owner = fs::metadata(&directory).unwrap();
    let time = NativeWirePrimitives;

    // execute every source outcome against disposable native acknowledgement and anchor files
    for case in &oracle.cases {
        let evidence = directory.join(&case.name);
        private_directory(&evidence);
        let acknowledgement_files = install_acknowledgements(&evidence, case, &oracle);
        let witness_path = evidence.join("epoch-witness.json");
        // retain the exact original witness through the real native file reader
        if case.witness != "-" {
            fs::write(&witness_path, &oracle.documents[&case.witness].bytes).unwrap();
            fs::set_permissions(&witness_path, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let anchor_root = evidence.join(&case.directory);
        let before = expected_files(&case.before, &oracle);
        // preserve the exact initial anchor directory and all three slots
        if case.present {
            private_directory(&anchor_root);
            for (filename, bytes) in &before {
                write_fixture(&anchor_root.join(filename), bytes);
            }
        } else {
            assert!(before.is_empty(), "{}", case.name);
        }
        let mut files = NativeCustodyAnchorInstallFiles::for_paths(
            evidence.clone(),
            witness_path.clone(),
            owner.uid(),
            owner.gid(),
            owner.uid(),
            owner.gid(),
        );
        let mut codec = NativeWirePrimitives;
        let mut input = Cursor::new(oracle.documents[&case.input].bytes.as_slice());
        let actual = install_custody_anchor_command(
            &case.action,
            &[case.argument.as_str()],
            &mut input,
            &mut files,
            &mut codec,
            &time,
            CustodyAnchorControl {
                sha256: optional(&case.control_sha),
                version: optional(&case.control_version),
            },
        );
        // compare the exact canonical installation receipt or original refusal
        match &case.expected {
            Some(expected) => assert_eq!(
                actual.expect(&case.name).as_deref(),
                Some(oracle.documents[expected].bytes.as_slice()),
                "{}",
                case.name
            ),
            None => assert!(actual.is_err(), "{}", case.name),
        }
        // compare the entire anchor namespace after the original commit or interrupted-state refusal
        let expected = expected_files(&case.after, &oracle);
        if case.after_present {
            let metadata = fs::metadata(&anchor_root).unwrap();
            assert!(metadata.is_dir(), "{}", case.name);
            assert_eq!(metadata.mode() & 0o777, 0o700, "{}", case.name);
            assert_eq!(read_files(&anchor_root), expected, "{}", case.name);
        } else {
            assert!(!anchor_root.exists(), "{}", case.name);
            assert!(expected.is_empty(), "{}", case.name);
        }
        // no install path may mutate retained cold acknowledgement or epoch authority bytes
        let ack_dir = evidence.join("revision-custody-acknowledgements");
        if ack_dir.exists() {
            assert_eq!(read_files(&ack_dir), acknowledgement_files, "{}", case.name);
        }
        if case.witness != "-" {
            assert_eq!(
                fs::read(&witness_path).unwrap(),
                oracle.documents[&case.witness].bytes,
                "{}",
                case.name
            );
        }
    }
}
