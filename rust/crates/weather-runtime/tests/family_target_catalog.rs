use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_runtime::{
    FamilyTargetCatalogInput, FamilyTargetImageRead, MaintenanceAnchorStatusError,
    MaintenanceAnchorStatusJson, MaintenanceCanonicalJson, build_family_target_catalog,
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
    family: String,
    source: String,
    environment: String,
    image_commit: String,
    settings: String,
    installed_at: String,
    custody: Option<String>,
    expected: Option<String>,
    clock_calls: usize,
}

struct Member {
    path: String,
    maximum: usize,
    document: Option<String>,
}

#[derive(Default)]
struct Oracle {
    documents: BTreeMap<String, Document>,
    hashes: BTreeMap<Vec<u8>, String>,
    clocks: BTreeMap<String, i64>,
    cases: Vec<Case>,
    members: BTreeMap<String, Vec<Member>>,
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
            .join("tests/family-target-catalog-node-oracle.mjs");
        let child = Command::new("node")
            .arg(script)
            .output()
            .expect("Node family target catalog oracle");
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
                [
                    "case",
                    name,
                    action,
                    action_sha256,
                    family,
                    source,
                    environment,
                    image_commit,
                    settings,
                    installed_at,
                    custody,
                    expected,
                    clock_calls,
                ] => oracle.cases.push(Case {
                    name: (*name).to_owned(),
                    action: (*action).to_owned(),
                    action_sha256: (*action_sha256).to_owned(),
                    family: (*family).to_owned(),
                    source: (*source).to_owned(),
                    environment: (*environment).to_owned(),
                    image_commit: (*image_commit).to_owned(),
                    settings: (*settings).to_owned(),
                    installed_at: (*installed_at).to_owned(),
                    custody: (*custody != "-").then(|| (*custody).to_owned()),
                    expected: (*expected != "error").then(|| (*expected).to_owned()),
                    clock_calls: clock_calls.parse().unwrap(),
                }),
                ["member", name, path, maximum, document] => {
                    oracle
                        .members
                        .entry((*name).to_owned())
                        .or_default()
                        .push(Member {
                            path: (*path).to_owned(),
                            maximum: maximum.parse().unwrap(),
                            document: (*document != "-").then(|| (*document).to_owned()),
                        });
                }
                _ => panic!("unexpected family target catalog oracle row: {row}"),
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
            .all(|byte| (byte.is_ascii_graphic() || byte == b' ') && !matches!(byte, b'"' | b'\\'))
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

struct Image<'a> {
    oracle: &'a Oracle,
    members: &'a [Member],
    index: &'a Cell<usize>,
    case: &'a str,
}

impl FamilyTargetImageRead for Image<'_> {
    // reproduce only the original image reads in their exact path, bound and call order
    fn read_member(
        &mut self,
        path: &str,
        maximum: usize,
    ) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        let member = self
            .members
            .get(self.index.get())
            .unwrap_or_else(|| panic!("{} unexpected member {path}", self.case));
        assert_eq!(path, member.path, "{}", self.case);
        assert_eq!(maximum, member.maximum, "{}", self.case);
        self.index.set(self.index.get() + 1);
        member
            .document
            .as_ref()
            .map(|name| self.oracle.documents[name].bytes.clone())
            .ok_or(MaintenanceAnchorStatusError::Slot)
    }
}

/// Compare both target builders with the exact original Node catalog bytes and image-read traces.
#[test]
fn family_target_catalog_matches_node_slot_transitions() {
    let oracle = Oracle::load();
    assert_eq!(oracle.cases.len(), 12);
    let time = Time(&oracle);
    // execute every actual original outcome using only its admitted immutable test member set
    for case in &oracle.cases {
        let action = fixture_node(&oracle.documents[&case.action], "$");
        let source = fixture_node(&oracle.documents[&case.source], "$");
        let environment = fixture_node(&oracle.documents[&case.environment], "$");
        let members: &[Member] = oracle.members.get(&case.name).map_or(&[], Vec::as_slice);
        let reads = Cell::new(0);
        let clock_calls = Cell::new(0);
        // sample the clock only after the exact original image member sequence
        let clock = || {
            assert_eq!(reads.get(), members.len(), "{}", case.name);
            clock_calls.set(clock_calls.get() + 1);
            Ok(case.installed_at.clone())
        };
        let input = FamilyTargetCatalogInput {
            action: &action,
            action_sha256: &case.action_sha256,
            family: &case.family,
            source: &source,
            environment: &environment,
            image_commit: &case.image_commit,
            settings_bytes: &oracle.documents[&case.settings].bytes,
            clock: &clock,
            custody_anchor_sha256: case.custody.as_deref(),
        };
        let mut image = Image {
            oracle: &oracle,
            members,
            index: &reads,
            case: &case.name,
        };
        let mut codec = Codec(&oracle);
        let result = build_family_target_catalog(&input, &mut image, &mut codec, &time);
        // compare exact canonical output or the equivalent original refusal
        match &case.expected {
            Some(expected) => assert_eq!(
                codec
                    .encode_canonical_json(&result.expect(&case.name))
                    .as_deref(),
                Some(oracle.documents[expected].bytes.as_slice()),
                "{}",
                case.name
            ),
            None => assert!(result.is_err(), "{}", case.name),
        }
        assert_eq!(reads.get(), members.len(), "{}", case.name);
        assert_eq!(clock_calls.get(), case.clock_calls, "{}", case.name);
    }
}
