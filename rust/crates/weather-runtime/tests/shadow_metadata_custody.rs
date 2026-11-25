use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Default)]
struct Document {
    bytes: Vec<u8>,
}

struct Case {
    name: String,
    action: String,
    argument: Option<String>,
    documents: [String; 5],
    expected: Option<Vec<u8>>,
    after_current: bool,
    after_preparation: bool,
}

#[derive(Default)]
struct Oracle {
    documents: BTreeMap<String, Document>,
    cases: Vec<Case>,
}

// retain the exact original LF framing of every observed fixture and result
fn line(value: &str) -> Vec<u8> {
    format!("{value}\n").into_bytes()
}

impl Oracle {
    // run the actual original Node operator against owned disposable files
    fn load() -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/shadow-metadata-custody-node-oracle.mjs");
        let child = Command::new("node")
            .arg(script)
            .output()
            .expect("Node custody oracle");
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        let mut oracle = Self::default();
        // preserve the decoded fixture tree and original command outcome separately
        for row in stdout.lines() {
            match row.split('\t').collect::<Vec<_>>().as_slice() {
                ["doc", name, bytes] => {
                    oracle
                        .documents
                        .entry((*name).to_owned())
                        .or_default()
                        .bytes = line(bytes);
                }
                // the actual native executable now derives typed values and digests itself
                ["node", _, _, _, _] | ["hash", _, _] | ["clock", _, _] => {}
                [
                    "case",
                    name,
                    action,
                    argument,
                    current,
                    preparation,
                    previous,
                    previous_preparation,
                    consumption,
                    expected,
                    after_current,
                    after_preparation,
                ] => {
                    oracle.cases.push(Case {
                        name: (*name).to_owned(),
                        action: match *action {
                            "status" => "shadow-metadata-custody-status-v1",
                            "consumed" => "shadow-metadata-custody-consumed-v1",
                            _ => panic!("unexpected custody oracle action"),
                        }
                        .to_owned(),
                        argument: (*argument != "-").then(|| (*argument).to_owned()),
                        documents: [
                            *current,
                            *preparation,
                            *previous,
                            *previous_preparation,
                            *consumption,
                        ]
                        .map(str::to_owned),
                        expected: (*expected != "error").then(|| line(expected)),
                        after_current: *after_current == "1",
                        after_preparation: *after_preparation == "1",
                    });
                }
                _ => panic!("unexpected custody oracle row"),
            }
        }
        oracle
    }
}

struct FixtureRoot(PathBuf);

impl Drop for FixtureRoot {
    // remove only the successfully allocated disposable fixture root
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// write only fixture-owned local documents at the original private mode
fn write_fixture(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// Compare both actual Rust file-backed operators and the exact post-recovery retained state.
#[test]
fn shadow_custody_readers_match_node_consumption_recovery() {
    let oracle = Oracle::load();
    assert_eq!(oracle.cases.len(), 9);
    let root = std::env::temp_dir().join(format!(
        "weather-shadow-custody-parity-{}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("unique fixture root");
    let fixture = FixtureRoot(root);
    let directory = fs::canonicalize(&fixture.0).unwrap();
    let names = [
        "current.json",
        "preparation.json",
        "previous.json",
        "previous-preparation.json",
        "previous-consumption.json",
    ];

    // run each source outcome with the same selected current and retained proof files
    for case in &oracle.cases {
        let evidence = directory.join(&case.name);
        fs::create_dir(&evidence).unwrap();
        let proof_root = evidence.join("shadow-metadata-custody-proofs");
        let present = case.documents.iter().any(|name| name != "-");
        let mut expected_files = BTreeMap::new();
        // leave the entirely absent custody root absent
        if present {
            fs::create_dir(&proof_root).unwrap();
            fs::set_permissions(&proof_root, fs::Permissions::from_mode(0o700)).unwrap();
            // retain exact original bytes for every selected document
            for (filename, name) in names.iter().zip(&case.documents) {
                // only selected documents exist on disk
                if name != "-" {
                    let bytes = oracle.documents[name].bytes.clone();
                    write_fixture(&proof_root.join(filename), &bytes);
                    expected_files.insert((*filename).to_owned(), bytes);
                }
            }
        }
        let output = Command::new(env!("CARGO_BIN_EXE_weather-evidence"))
            .arg(&case.action)
            .args(case.argument.as_deref())
            .env("WEATHER_ADJUSTMENT_EVIDENCE_ROOT", &evidence)
            .output()
            .unwrap();
        // compare the actual executable output and retained recovery against the original
        match &case.expected {
            Some(expected) => {
                assert!(
                    output.status.success(),
                    "{}: {}",
                    case.name,
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(&output.stdout, expected, "{}", case.name);
            }
            None => assert!(!output.status.success(), "{}", case.name),
        }
        // retain each exact post-recovery marker that the original command retained
        if !case.after_current {
            expected_files.remove("current.json");
        }
        if !case.after_preparation {
            expected_files.remove("preparation.json");
        }
        // do not invent a directory or remove any retained terminal proof document
        if present {
            let actual_files: BTreeMap<_, _> = fs::read_dir(&proof_root)
                .unwrap()
                .map(|entry| {
                    let path = entry.unwrap().path();
                    (
                        path.file_name().unwrap().to_str().unwrap().to_owned(),
                        fs::read(path).unwrap(),
                    )
                })
                .collect();
            assert_eq!(actual_files, expected_files, "{}", case.name);
        } else {
            assert!(!proof_root.exists(), "{}", case.name);
        }
    }
}
