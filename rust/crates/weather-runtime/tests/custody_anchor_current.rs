use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_runtime::{
    MaintenanceAnchorStatusJson, MaintenanceCanonicalJson, NativeCustodyAnchorCurrentFiles,
    read_custody_anchor_current_command,
};

const SLOT_NAMES: [&str; 3] = ["current.json", "pending.json", "previous.json"];

#[derive(Default)]
struct Document {
    bytes: Vec<u8>,
    nodes: BTreeMap<String, (String, String)>,
}

struct Case {
    name: String,
    action: String,
    directory: String,
    present: bool,
    before: [String; 3],
    expected: Option<String>,
    after: [String; 3],
}

#[derive(Default)]
struct Oracle {
    documents: BTreeMap<String, Document>,
    hashes: BTreeMap<Vec<u8>, String>,
    clocks: BTreeMap<String, i64>,
    cases: Vec<Case>,
}

// retain the exact one-lf framing used by both original current commands
fn line(value: &str) -> Vec<u8> {
    format!("{value}\n").into_bytes()
}

// decode one original Node-computed SHA-256 preimage
fn decode_hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

// encode one original fixture scalar in the generated JSON representation
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

impl Oracle {
    // invoke the actual original Node commands and retain only their closed fixture protocol
    fn load() -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/custody-anchor-current-node-oracle.mjs");
        let child = Command::new("node")
            .arg(script)
            .output()
            .expect("Node custody anchor current oracle");
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        let mut oracle = Self::default();
        // decode every source document, digest, clock and command outcome exactly once
        for row in stdout.lines() {
            match row.split('\t').collect::<Vec<_>>().as_slice() {
                ["doc", name, bytes] => {
                    oracle
                        .documents
                        .entry((*name).to_owned())
                        .or_default()
                        .bytes = line(bytes);
                }
                ["node", name, path, kind, scalar] => {
                    oracle
                        .documents
                        .entry((*name).to_owned())
                        .or_default()
                        .nodes
                        .insert(
                            (*path).to_owned(),
                            ((*kind).to_owned(), (*scalar).to_owned()),
                        );
                }
                ["hash", preimage, digest] => {
                    oracle
                        .hashes
                        .insert(decode_hex(preimage), (*digest).to_owned());
                }
                ["clock", instant, epoch] => {
                    oracle
                        .clocks
                        .insert((*instant).to_owned(), epoch.parse().unwrap());
                }
                [
                    "case",
                    name,
                    action,
                    present,
                    current,
                    pending,
                    previous,
                    expected,
                    after_current,
                    after_pending,
                    after_previous,
                ] => {
                    let (action, directory) = match *action {
                        "dev" => (
                            "development-custody-anchor-current-v1",
                            "development-custody-anchors",
                        ),
                        "rain" => (
                            "rain-control-custody-anchor-current-v1",
                            "rain-control-custody-anchors",
                        ),
                        _ => panic!("unexpected custody anchor action"),
                    };
                    oracle.cases.push(Case {
                        name: (*name).to_owned(),
                        action: action.to_owned(),
                        directory: directory.to_owned(),
                        present: *present == "present",
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

// reconstruct only one decoded fixture value supplied by the actual Node oracle
fn fixture_node(document: &Document, path: &str) -> JsonValue {
    let (kind, scalar) = &document.nodes[path];
    let prefix = format!("{path}/");
    // retain only immediate children so nested keys are not duplicated
    let children = || {
        document.nodes.keys().filter_map(|key| {
            let suffix = key.strip_prefix(&prefix)?;
            (!suffix.contains('/')).then_some((suffix, key.as_str()))
        })
    };
    match kind.as_str() {
        "object" => JsonValue::Object(
            children()
                .map(|(key, child)| (key.encode_utf16().collect(), fixture_node(document, child)))
                .collect(),
        ),
        "array" => {
            let mut indexed: Vec<_> = children()
                .map(|(index, child)| (index.parse::<usize>().unwrap(), child))
                .collect();
            indexed.sort_by_key(|(index, _)| *index);
            JsonValue::Array(
                indexed
                    .into_iter()
                    .map(|(_, child)| fixture_node(document, child))
                    .collect(),
            )
        }
        "string" => string(scalar),
        "number" => JsonValue::Number(scalar.parse().unwrap()),
        "null" => JsonValue::Null,
        "boolean" => JsonValue::Boolean(scalar == "true"),
        _ => panic!("unexpected fixture kind"),
    }
}

// quote only the fixture's known ASCII scalar alphabet
fn quoted(value: &str) -> String {
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
    );
    format!("\"{value}\"")
}

// independently encode the test-only canonical fixture subset
fn fixture_json(value: &JsonValue) -> String {
    match value {
        JsonValue::Boolean(value) => value.to_string(),
        JsonValue::Number(value) => {
            assert!(value.is_finite() && value.fract() == 0.0);
            value.to_string()
        }
        JsonValue::String(units) => quoted(&String::from_utf16(units).unwrap()),
        JsonValue::Null => "null".to_owned(),
        JsonValue::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(fixture_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        JsonValue::Object(fields) => {
            let mut fields: Vec<_> = fields.iter().collect();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            format!(
                "{{{}}}",
                fields
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        quoted(&String::from_utf16(key).unwrap()),
                        fixture_json(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

struct Codec<'a>(&'a Oracle);

impl MaintenanceCanonicalJson for Codec<'_> {
    // admit only one exact original canonical document
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        let document = self
            .0
            .documents
            .values()
            .find(|value| value.bytes == bytes)?;
        Some(fixture_node(document, "."))
    }

    // independently encode the bounded fixture value subset
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        Some(line(&fixture_json(value)))
    }

    // match only exact original Node-computed hash preimages
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.0.hashes.get(bytes).map(Sha256Digest::admitted)
    }
}

impl MaintenanceAnchorStatusJson for Codec<'_> {
    // preserve the original JavaScript String coercions used by regex fields
    fn js_string(&mut self, value: &JsonValue) -> Option<Vec<u16>> {
        match value {
            JsonValue::String(units) => Some(units.clone()),
            JsonValue::Number(value) => Some(value.to_string().encode_utf16().collect()),
            JsonValue::Array(values) if values.len() == 1 => self.js_string(&values[0]),
            _ => None,
        }
    }
}

struct Time<'a>(&'a Oracle);

impl CurrentTimeResolver for Time<'_> {
    // resolve only exact canonical original Node fixture instants
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let JsDateOrString::String(units) = value else {
            panic!("fixture instant must be a string")
        };
        let utc = String::from_utf16(units).unwrap();
        Ok(CanonicalInstant {
            epoch_milliseconds: self.0.clocks[&utc],
            utc,
        })
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

/// Compare both native Rust current readers with exact original Node command outcomes.
#[test]
fn custody_anchor_current_readers_match_original_node_and_leave_state_unchanged() {
    let oracle = Oracle::load();
    assert_eq!(oracle.cases.len(), 16);
    assert_eq!(oracle.documents.len(), 15);
    assert_eq!(oracle.hashes.len(), 4);
    assert_eq!(oracle.clocks.len(), 3);
    let root = std::env::temp_dir().join(format!(
        "weather-custody-anchor-current-parity-{}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("unique fixture root");
    let fixture = FixtureRoot(root);
    let directory = fs::canonicalize(&fixture.0).unwrap();
    let owner = fs::metadata(&directory).unwrap();
    let time = Time(&oracle);

    // exercise every original source outcome against an isolated file-backed Rust owner
    for case in &oracle.cases {
        let evidence = directory.join(&case.name);
        fs::create_dir(&evidence).unwrap();
        let anchor_root = evidence.join(&case.directory);
        let before = expected_files(&case.before, &oracle);
        // preserve genuine absence separately from an existing empty private directory
        if case.present {
            fs::create_dir(&anchor_root).unwrap();
            fs::set_permissions(&anchor_root, fs::Permissions::from_mode(0o700)).unwrap();
            // populate only the source-selected fixed slots
            for (filename, bytes) in &before {
                write_fixture(&anchor_root.join(filename), bytes);
            }
        }
        let mut files =
            NativeCustodyAnchorCurrentFiles::for_evidence_root(evidence, owner.uid(), owner.gid());
        let mut codec = Codec(&oracle);
        let actual =
            read_custody_anchor_current_command(&case.action, &[], &mut files, &mut codec, &time);
        // compare exact canonical stdout or the equivalent original refusal
        match &case.expected {
            Some(expected) => assert_eq!(
                actual.expect(&case.name).as_deref(),
                Some(oracle.documents[expected].bytes.as_slice()),
                "{}",
                case.name
            ),
            None => assert!(actual.is_err(), "{}", case.name),
        }
        // neither command may install, commit, clean or alter any fixed slot
        if case.present {
            let metadata = fs::metadata(&anchor_root).unwrap();
            assert!(metadata.is_dir(), "{}", case.name);
            assert_eq!(metadata.mode() & 0o777, 0o700, "{}", case.name);
            assert_eq!(
                read_files(&anchor_root),
                expected_files(&case.after, &oracle),
                "{}",
                case.name
            );
            assert_eq!(
                expected_files(&case.after, &oracle),
                before,
                "{}",
                case.name
            );
        } else {
            assert!(!anchor_root.exists(), "{}", case.name);
        }
    }
}
