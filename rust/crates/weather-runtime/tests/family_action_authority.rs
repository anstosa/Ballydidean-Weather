use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_runtime::{
    MaintenanceAnchorStatusJson, MaintenanceCanonicalJson, NativeFamilyActionAuthorityFiles,
    authorize_family_action,
};

#[derive(Default)]
struct Document {
    bytes: Vec<u8>,
    nodes: BTreeMap<String, (String, String)>,
}

struct Case {
    name: String,
    action: String,
    action_sha256: String,
    kind: String,
    current: Option<String>,
    expected: Option<String>,
}

#[derive(Default)]
struct Oracle {
    documents: BTreeMap<String, Document>,
    hashes: BTreeMap<Vec<u8>, String>,
    clocks: BTreeMap<String, i64>,
    cases: Vec<Case>,
}

#[derive(Debug, Eq, PartialEq)]
struct SnapshotEntry {
    kind: &'static str,
    mode: u32,
    uid: u32,
    gid: u32,
    bytes: Vec<u8>,
}

// preserve the predecessor's exact one-lf canonical document framing
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
    // invoke the original Node authorities and retain only their closed fixture protocol
    fn load() -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/family-action-authority-node-oracle.mjs");
        // opt into the root-mapped wrapper oracle only on hosts that support user namespaces
        let mut command = match std::env::var_os("WEATHER_FAMILY_AUTHORITY_NODE_USERNS").as_deref()
        {
            Some(value) if value == OsStr::new("1") => {
                let mut command = Command::new("unshare");
                command.args(["--user", "--map-root-user", "node"]);
                command
            }
            _ => Command::new("node"),
        };
        let child = command
            .arg(script)
            .output()
            .expect("Node family action authority oracle");
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        let mut oracle = Self::default();
        // decode every source document, digest, clock and authorization outcome exactly once
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
                ["case", name, action, action_sha256, kind, current, expected] => {
                    oracle.cases.push(Case {
                        name: (*name).to_owned(),
                        action: (*action).to_owned(),
                        action_sha256: (*action_sha256).to_owned(),
                        kind: (*kind).to_owned(),
                        current: (*current != "-").then(|| (*current).to_owned()),
                        expected: (*expected != "error").then(|| (*expected).to_owned()),
                    })
                }
                _ => panic!("unexpected family action authority oracle row: {row}"),
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
        Some(fixture_node(document, "$"))
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

// select only the fixed family authority root named by the oracle
fn anchor_directory(kind: &str) -> &'static str {
    match kind {
        "dev" => "development-custody-anchors",
        "rain" => "rain-control-custody-anchors",
        "maintenance" => "maintenance-anchors",
        _ => panic!("unexpected family action authority kind"),
    }
}

// write one fixture-owned exact private current file
fn write_current(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

// snapshot the complete fixture namespace without following symbolic links
fn snapshot(root: &Path) -> BTreeMap<PathBuf, SnapshotEntry> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    // retain every directory and ordinary-file identity beneath the case root
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        let relative = path.strip_prefix(root).unwrap().to_path_buf();
        // descend through each fixture-owned directory in stable name order
        let (kind, bytes) = if metadata.is_dir() {
            let mut children = fs::read_dir(&path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            pending.extend(children.into_iter().rev());
            ("directory", Vec::new())
        } else {
            // retain the complete contents of every ordinary fixture file
            assert!(metadata.is_file(), "fixture contains a non-file entry");
            ("file", fs::read(&path).unwrap())
        };
        entries.insert(
            relative,
            SnapshotEntry {
                kind,
                mode: metadata.mode() & 0o777,
                uid: metadata.uid(),
                gid: metadata.gid(),
                bytes,
            },
        );
    }
    entries
}

/// Compare the native family dispatcher with the original Node authorization outcomes.
#[test]
fn family_action_authority_matches_original_node_and_leaves_state_unchanged() {
    let oracle = Oracle::load();
    assert_eq!(oracle.cases.len(), 13);
    assert_eq!(oracle.documents.len(), 15);
    assert_eq!(
        oracle
            .documents
            .values()
            .map(|document| document.nodes.len())
            .sum::<usize>(),
        220
    );
    assert_eq!(oracle.hashes.len(), 89);
    assert_eq!(oracle.clocks.len(), 3);
    let root = std::env::temp_dir().join(format!(
        "weather-family-action-authority-parity-{}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("unique fixture root");
    let fixture = FixtureRoot(root);
    let directory = fs::canonicalize(&fixture.0).unwrap();
    let owner = fs::metadata(&directory).unwrap();
    let time = Time(&oracle);

    // exercise every original outcome through an isolated file-backed Rust owner
    for case in &oracle.cases {
        let evidence = directory.join(&case.name);
        fs::create_dir(&evidence).unwrap();
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700)).unwrap();
        // preserve genuine absence separately from one present private current anchor
        if let Some(current) = &case.current {
            let anchor_root = evidence.join(anchor_directory(&case.kind));
            fs::create_dir(&anchor_root).unwrap();
            fs::set_permissions(&anchor_root, fs::Permissions::from_mode(0o700)).unwrap();
            write_current(
                &anchor_root.join("current.json"),
                &oracle.documents[current].bytes,
            );
        }
        let before = snapshot(&evidence);
        let action = fixture_node(&oracle.documents[&case.action], "$");
        let mut files = NativeFamilyActionAuthorityFiles::for_evidence_root(
            evidence.clone(),
            owner.uid(),
            owner.gid(),
            owner.uid(),
        );
        let mut codec = Codec(&oracle);
        let actual =
            authorize_family_action(&action, &case.action_sha256, &mut files, &mut codec, &time);
        // compare the exact canonical returned anchor or the equivalent original refusal
        match &case.expected {
            Some(expected) => assert_eq!(
                codec
                    .encode_canonical_json(&actual.expect(&case.name))
                    .as_deref(),
                Some(oracle.documents[expected].bytes.as_slice()),
                "{}",
                case.name
            ),
            None => assert!(actual.is_err(), "{}", case.name),
        }
        assert_eq!(snapshot(&evidence), before, "{}", case.name);
    }
}
