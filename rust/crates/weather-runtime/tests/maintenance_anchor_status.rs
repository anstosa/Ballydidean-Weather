use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_runtime::{
    MaintenanceAnchorStatusJson, MaintenanceCanonicalJson, NativeMaintenanceAnchorStatusFiles,
    read_maintenance_anchor_status_command,
};

#[derive(Default)]
struct Anchor {
    bytes: Vec<u8>,
    checksum: String,
    fields: BTreeMap<String, JsonValue>,
}

struct Case {
    name: String,
    present: bool,
    slots: [String; 3],
    extra: bool,
    expected: Option<Vec<u8>>,
}

#[derive(Default)]
struct Oracle {
    anchors: BTreeMap<String, Anchor>,
    clocks: BTreeMap<String, i64>,
    cases: Vec<Case>,
}

// preserve the original one-lf canonical fixture bytes
fn line(value: &str) -> Vec<u8> {
    format!("{value}\n").into_bytes()
}

// encode one fixture string in the generated JSON's UTF-16 representation
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

impl Oracle {
    // run the original Node command against its own disposable current-user file fixtures
    fn read() -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/maintenance-anchor-status-node-oracle.mjs");
        let output = Command::new("node")
            .arg(script)
            .output()
            .expect("Node oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("oracle output");
        let mut oracle = Self::default();
        // decode only the bounded original fixture fields rather than implementing a production codec
        for row in stdout.lines() {
            match row.split('\t').collect::<Vec<_>>().as_slice() {
                ["anchor", name, bytes, digest] => {
                    let anchor = oracle.anchors.entry((*name).to_owned()).or_default();
                    anchor.bytes = line(bytes);
                    anchor.checksum = (*digest).to_owned();
                }
                ["field", name, field, kind, value] => {
                    let value = match *kind {
                        "string" => string(value),
                        "number" => JsonValue::Number(value.parse().unwrap()),
                        "null" => JsonValue::Null,
                        "array" => JsonValue::Array(vec![string(value)]),
                        _ => panic!("unexpected fixture kind"),
                    };
                    oracle
                        .anchors
                        .get_mut(*name)
                        .unwrap()
                        .fields
                        .insert((*field).to_owned(), value);
                }
                ["clock", clock, epoch] => {
                    oracle
                        .clocks
                        .insert((*clock).to_owned(), epoch.parse().unwrap());
                }
                [
                    "case",
                    name,
                    present,
                    current,
                    pending,
                    previous,
                    extra,
                    result,
                ] => {
                    oracle.cases.push(Case {
                        name: (*name).to_owned(),
                        present: *present == "present",
                        slots: [*current, *pending, *previous].map(str::to_owned),
                        extra: *extra == "extra",
                        expected: (*result != "error").then(|| line(result)),
                    });
                }
                _ => panic!("unexpected Node oracle row"),
            }
        }
        oracle
    }
}

// serialize only the known test fixture ASCII primitives without making a production encoder
fn fixture_json(value: &JsonValue) -> String {
    match value {
        JsonValue::Boolean(value) => value.to_string(),
        JsonValue::Number(value) => {
            assert!(value.is_finite() && value.fract() == 0.0);
            value.to_string()
        }
        JsonValue::String(units) => fixture_quoted(&String::from_utf16(units).unwrap()),
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
            let mut sorted: Vec<_> = fields.iter().collect();
            sorted.sort_by(|left, right| left.0.cmp(&right.0));
            let items = sorted
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        fixture_quoted(&String::from_utf16(key).unwrap()),
                        fixture_json(value)
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", items.join(","))
        }
    }
}

// admit only fixture strings that need no escaping
fn fixture_quoted(value: &str) -> String {
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
    );
    format!("\"{value}\"")
}

struct Codec<'a>(&'a Oracle);

impl MaintenanceCanonicalJson for Codec<'_> {
    // expose the already decoded original fixture only for its exact original bytes
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        let anchor = self
            .0
            .anchors
            .values()
            .find(|anchor| anchor.bytes == bytes)?;
        Some(JsonValue::Object(
            anchor
                .fields
                .iter()
                .map(|(key, value)| (key.encode_utf16().collect(), value.clone()))
                .collect(),
        ))
    }

    // serialize the independent small Rust projection in the fixture's known canonical subset
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        Some(line(&fixture_json(value)))
    }

    // hash only exact original Node fixture material through the admitted checksum witness
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.0
            .anchors
            .values()
            .find(|anchor| anchor.bytes == bytes)
            .map(|anchor| Sha256Digest::admitted(&anchor.checksum))
    }
}

impl MaintenanceAnchorStatusJson for Codec<'_> {
    // retain the two actual JSON scalar coercions exercised by the predecessor regex validators
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
    // resolve only the original exact captured UTC dates
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let JsDateOrString::String(units) = value else {
            panic!("expected fixture instant")
        };
        let utc = String::from_utf16(units).expect("fixture time");
        Ok(CanonicalInstant {
            epoch_milliseconds: self.0.clocks[&utc],
            utc,
        })
    }
}

struct FixtureRoot(PathBuf);

impl Drop for FixtureRoot {
    // remove only the successfully created disposable test directory
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// write only ordinary current-user fixture files with the predecessor's exact mode
fn fixture_slot(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// Exercise the actual Rust native read owner and full v2/v3 schema against Node's status bytes.
#[test]
fn readonly_anchor_status_matches_the_original_node_command() {
    let oracle = Oracle::read();
    assert_eq!(oracle.cases.len(), 9);
    let path = std::env::temp_dir().join(format!(
        "weather-anchor-status-parity-{}",
        std::process::id()
    ));
    fs::create_dir(&path).expect("unique disposable integration root");
    let fixture = FixtureRoot(path);
    let root = fs::canonicalize(&fixture.0).unwrap();
    let owner = fs::metadata(&root).unwrap();
    let slot_names = ["current.json", "pending.json", "previous.json"];
    let time = Time(&oracle);

    // compare each original status and assert the read-only command leaves the fixture unchanged
    for case in &oracle.cases {
        let evidence = root.join(&case.name);
        fs::create_dir(&evidence).unwrap();
        let directory = evidence.join("maintenance-anchors");
        let mut original = BTreeMap::new();
        // build only the selected fixture state
        if case.present {
            fs::create_dir(&directory).unwrap();
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            // retain the original fixed slot contents for post-command comparison
            for (filename, name) in slot_names.iter().zip(&case.slots) {
                // do not create absent slots
                if name != "-" {
                    let bytes = oracle.anchors[name].bytes.clone();
                    fixture_slot(&directory.join(filename), &bytes);
                    original.insert((*filename).to_owned(), bytes);
                }
            }
            // retain one ordinary unexpected entry test fixture
            if case.extra {
                fixture_slot(&directory.join("unexpected.txt"), b"fixture");
                original.insert("unexpected.txt".to_owned(), b"fixture".to_vec());
            }
        }
        let mut files = NativeMaintenanceAnchorStatusFiles::for_evidence_root(
            evidence,
            owner.uid(),
            owner.gid(),
        );
        let mut codec = Codec(&oracle);
        let result = read_maintenance_anchor_status_command(
            "maintenance-anchor-status-v2",
            &[],
            &mut files,
            &mut codec,
            &time,
        );
        // compare exact successful stdout or an ordinary source-defined refusal
        match &case.expected {
            Some(expected) => assert_eq!(
                result.expect(&case.name).as_deref(),
                Some(expected.as_slice()),
                "{}",
                case.name
            ),
            None => assert!(result.is_err(), "{}", case.name),
        }
        // the command must not create a root, alter any file or complete pending state
        if case.present {
            let after: BTreeMap<_, _> = fs::read_dir(&directory)
                .unwrap()
                .map(|entry| {
                    let path = entry.unwrap().path();
                    (
                        path.file_name().unwrap().to_str().unwrap().to_owned(),
                        fs::read(path).unwrap(),
                    )
                })
                .collect();
            assert_eq!(after, original, "{}", case.name);
        } else {
            assert!(!directory.exists());
        }
    }
}
