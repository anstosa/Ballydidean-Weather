use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_runtime::{
    MaintenanceAnchorStatusJson, MaintenanceCanonicalJson, NativeShadowMetadataCustodyFiles,
    ShadowMetadataCustodyOwnerJson, ShadowMetadataCustodyPublishInput,
    commit_shadow_metadata_custody_proof, mutate_shadow_metadata_custody_command,
    prepare_shadow_metadata_custody_proof, shadow_metadata_custody_owner_command,
};

const SLOT_NAMES: [&str; 5] = [
    "current.json",
    "preparation.json",
    "previous.json",
    "previous-preparation.json",
    "previous-consumption.json",
];

#[derive(Default)]
struct Document {
    bytes: Vec<u8>,
    nodes: BTreeMap<String, (String, String)>,
}

struct Case {
    name: String,
    action: String,
    now: String,
    argument: String,
    input: String,
    before: [String; 5],
    expected: Option<String>,
    after: [String; 5],
}

struct RawDocument {
    typed: String,
    bytes: Vec<u8>,
}

struct OwnerQueryCase {
    name: String,
    action: String,
    argument: String,
    before: [String; 2],
    expected: Option<Vec<u8>>,
}

struct OwnerMutationCase {
    name: String,
    action: String,
    now: String,
    argument: String,
    input: String,
    before: [String; 5],
    expected: Option<String>,
    after: [String; 5],
}

#[derive(Default)]
struct Oracle {
    documents: BTreeMap<String, Document>,
    raw_documents: BTreeMap<String, RawDocument>,
    hashes: BTreeMap<Vec<u8>, String>,
    clocks: BTreeMap<String, i64>,
    cases: Vec<Case>,
    owner_queries: Vec<OwnerQueryCase>,
    owner_mutations: Vec<OwnerMutationCase>,
}

// retain the exact one-lf framing used by the original private files and command input
fn line(value: &str) -> Vec<u8> {
    format!("{value}\n").into_bytes()
}

// encode one original fixture scalar in the generated JSON representation
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
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

impl Oracle {
    // invoke the actual original Node commands and retain their typed fixtures
    fn load() -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/shadow-metadata-custody-mutation-node-oracle.mjs");
        let child = Command::new("node")
            .arg(script)
            .output()
            .expect("Node custody mutation oracle");
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        let mut oracle = Self::default();
        // decode only the fixed tab-separated oracle protocol
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
                ["clock", clock, epoch] => {
                    oracle
                        .clocks
                        .insert((*clock).to_owned(), epoch.parse().unwrap());
                }
                ["raw", name, typed, bytes] => {
                    oracle.raw_documents.insert(
                        (*name).to_owned(),
                        RawDocument {
                            typed: (*typed).to_owned(),
                            bytes: decode_hex(bytes),
                        },
                    );
                }
                [
                    "owner-query",
                    name,
                    action,
                    argument,
                    current,
                    preparation,
                    expected,
                ] => {
                    oracle.owner_queries.push(OwnerQueryCase {
                        name: (*name).to_owned(),
                        action: (*action).to_owned(),
                        argument: (*argument).to_owned(),
                        before: [*current, *preparation].map(str::to_owned),
                        expected: (*expected != "error").then(|| decode_hex(expected)),
                    });
                }
                [
                    "owner-mutation",
                    name,
                    action,
                    now,
                    argument,
                    input,
                    current,
                    preparation,
                    previous,
                    previous_preparation,
                    consumption,
                    expected,
                    after_current,
                    after_preparation,
                    after_previous,
                    after_previous_preparation,
                    after_consumption,
                ] => {
                    oracle.owner_mutations.push(OwnerMutationCase {
                        name: (*name).to_owned(),
                        action: (*action).to_owned(),
                        now: (*now).to_owned(),
                        argument: (*argument).to_owned(),
                        input: (*input).to_owned(),
                        before: [
                            *current,
                            *preparation,
                            *previous,
                            *previous_preparation,
                            *consumption,
                        ]
                        .map(str::to_owned),
                        expected: (*expected != "error").then(|| (*expected).to_owned()),
                        after: [
                            *after_current,
                            *after_preparation,
                            *after_previous,
                            *after_previous_preparation,
                            *after_consumption,
                        ]
                        .map(str::to_owned),
                    });
                }
                [
                    "case",
                    name,
                    action,
                    now,
                    argument,
                    input,
                    current,
                    preparation,
                    previous,
                    previous_preparation,
                    consumption,
                    expected,
                    after_current,
                    after_preparation,
                    after_previous,
                    after_previous_preparation,
                    after_consumption,
                ] => {
                    oracle.cases.push(Case {
                        name: (*name).to_owned(),
                        action: match *action {
                            "prepare" => "shadow-metadata-custody-prepare-v1",
                            "consume" => "shadow-metadata-custody-consume-v1",
                            _ => panic!("unexpected custody mutation action"),
                        }
                        .to_owned(),
                        now: (*now).to_owned(),
                        argument: (*argument).to_owned(),
                        input: (*input).to_owned(),
                        before: [
                            *current,
                            *preparation,
                            *previous,
                            *previous_preparation,
                            *consumption,
                        ]
                        .map(str::to_owned),
                        expected: (*expected != "error").then(|| (*expected).to_owned()),
                        after: [
                            *after_current,
                            *after_preparation,
                            *after_previous,
                            *after_previous_preparation,
                            *after_consumption,
                        ]
                        .map(str::to_owned),
                    });
                }
                _ => panic!("unexpected custody mutation oracle row: {row}"),
            }
        }
        oracle
    }
}

// reconstruct only one decoded fixture object supplied by the original Node oracle
fn fixture_node(document: &Document, path: &str) -> JsonValue {
    let (kind, scalar) = &document.nodes[path];
    let prefix = format!("{path}/");
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

// select one exact field from an oracle-generated fixture object
fn fixture_field<'a>(value: &'a JsonValue, name: &str) -> &'a JsonValue {
    let JsonValue::Object(fields) = value else {
        panic!("fixture value must be an object")
    };
    let key: Vec<u16> = name.encode_utf16().collect();
    &fields.iter().find(|(field, _)| field == &key).unwrap().1
}

// decode one exact string field from an oracle-generated fixture object
fn fixture_text(value: &JsonValue, name: &str) -> String {
    let JsonValue::String(units) = fixture_field(value, name) else {
        panic!("fixture field must be a string")
    };
    String::from_utf16(units).unwrap()
}

// project the already-admitted proof fixture into the native writer boundary
fn publish_input(proof: &JsonValue) -> ShadowMetadataCustodyPublishInput {
    let JsonValue::Array(entries) = fixture_field(proof, "entries") else {
        panic!("fixture entries must be an array")
    };
    ShadowMetadataCustodyPublishInput {
        acknowledged_at: fixture_text(proof, "preparedAt"),
        acknowledgement_sha256: Sha256Digest::admitted(fixture_text(
            proof,
            "acknowledgementSha256",
        )),
        archive_commit_ordinal: fixture_text(proof, "archiveCommitOrdinal").parse().unwrap(),
        custody_checkpoint_sha256: Sha256Digest::admitted(fixture_text(
            proof,
            "custodyCheckpointSha256",
        )),
        entries: entries.clone(),
        frontier_sha256: Sha256Digest::admitted(fixture_text(proof, "frontierSha256")),
        member_root_sha256: Sha256Digest::admitted(fixture_text(proof, "memberRootSha256")),
        page_sha256: Sha256Digest::admitted(fixture_text(proof, "pageSha256")),
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
    // admit only exact original Node fixture bytes
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        let document = self
            .0
            .documents
            .values()
            .find(|document| document.bytes == bytes)?;
        Some(fixture_node(document, "."))
    }

    // independently encode exact fixture projections and unsigned documents
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        Some(line(&fixture_json(value)))
    }

    // match only exact original Node-computed preimages
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.0.hashes.get(bytes).map(Sha256Digest::admitted)
    }
}

impl MaintenanceAnchorStatusJson for Codec<'_> {
    // preserve exact fixture scalar and single-member array coercions
    fn js_string(&mut self, value: &JsonValue) -> Option<Vec<u16>> {
        match value {
            JsonValue::String(units) => Some(units.clone()),
            JsonValue::Number(value) => Some(value.to_string().encode_utf16().collect()),
            JsonValue::Array(values) if values.len() == 1 => self.js_string(&values[0]),
            _ => None,
        }
    }
}

impl ShadowMetadataCustodyOwnerJson for Codec<'_> {
    // map only exact noncanonical bytes emitted by the original owner bridge to their typed value
    fn parse_owner_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        let raw = self
            .0
            .raw_documents
            .values()
            .find(|document| document.bytes == bytes)?;
        Some(fixture_node(&self.0.documents[&raw.typed], "."))
    }
}

struct Time<'a>(&'a Oracle);

impl CurrentTimeResolver for Time<'_> {
    // resolve only the canonical original Node fixture instants
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let JsDateOrString::String(units) = value else {
            panic!("fixture date must be a string")
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
    // remove only this test's disposable fixture tree
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// write one fixture-owned exact private file
fn write_fixture(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

// read the complete namespace and require every retained entry to remain a private regular file
fn read_files(proof_root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(proof_root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            assert!(metadata.is_file());
            assert_eq!(metadata.mode() & 0o777, 0o600);
            (
                entry.file_name().into_string().unwrap(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

// create one exact source namespace beneath the already private evidence root
fn install_state(evidence: &Path, names: &[String; 5], oracle: &Oracle) -> PathBuf {
    let proof_root = evidence.join("shadow-metadata-custody-proofs");
    // retain genuine absence when the original source case begins without custody state
    if names.iter().any(|name| name != "-") {
        fs::create_dir(&proof_root).unwrap();
        fs::set_permissions(&proof_root, fs::Permissions::from_mode(0o700)).unwrap();
        // populate every exact selected source slot
        for (filename, name) in SLOT_NAMES.iter().zip(names) {
            if name != "-" {
                write_fixture(&proof_root.join(filename), &oracle.documents[name].bytes);
            }
        }
    }
    proof_root
}

// project one exact expected namespace from its five oracle document names
fn expected_state(names: &[String; 5], oracle: &Oracle) -> BTreeMap<String, Vec<u8>> {
    SLOT_NAMES
        .iter()
        .zip(names)
        .filter(|(_, name)| *name != "-")
        .map(|(filename, name)| ((*filename).to_owned(), oracle.documents[name].bytes.clone()))
        .collect()
}

// compare every resulting entry, retained byte and private file mode
fn assert_state(proof_root: &Path, expected: BTreeMap<String, Vec<u8>>, case: &str) {
    // preserve an entirely absent proof root when no command needed to create it
    if expected.is_empty() && !proof_root.exists() {
        return;
    }
    let proof_root_metadata = fs::metadata(proof_root).unwrap();
    assert!(proof_root_metadata.is_dir(), "{case}");
    assert_eq!(proof_root_metadata.mode() & 0o777, 0o700, "{case}");
    assert_eq!(read_files(proof_root), expected, "{case}");
}

/// Compare both file-backed Rust mutation commands with the original Node transitions.
#[test]
fn shadow_custody_mutations_match_node_outputs_and_retained_state() {
    let oracle = Oracle::load();
    assert_eq!(oracle.cases.len(), 11);
    assert_eq!(oracle.raw_documents.len(), 3);
    assert_eq!(oracle.owner_queries.len(), 10);
    assert_eq!(oracle.owner_mutations.len(), 3);
    let root = std::env::temp_dir().join(format!(
        "weather-shadow-custody-mutation-parity-{}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("unique fixture root");
    let fixture = FixtureRoot(root);
    let directory = fs::canonicalize(&fixture.0).unwrap();
    let owner = fs::metadata(&directory).unwrap();
    let time = weather_runtime::NativeWirePrimitives;

    // execute every original source outcome in an isolated current-user tree
    for case in &oracle.cases {
        let evidence = directory.join(&case.name);
        fs::create_dir(&evidence).unwrap();
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700)).unwrap();
        let proof_root = install_state(&evidence, &case.before, &oracle);
        let mut files =
            NativeShadowMetadataCustodyFiles::for_evidence_root(evidence, owner.uid(), owner.gid());
        let mut codec = weather_runtime::NativeWirePrimitives;
        let mut input = Cursor::new(oracle.documents[&case.input].bytes.as_slice());
        let actual = mutate_shadow_metadata_custody_command(
            &case.action,
            &[case.argument.as_str()],
            &mut input,
            &mut files,
            &mut codec,
            &time,
            &case.now,
        );
        // compare exact canonical successful output or equivalent refusal
        match &case.expected {
            Some(expected) => assert_eq!(
                actual.expect(&case.name).as_deref(),
                Some(oracle.documents[expected].bytes.as_slice()),
                "{}",
                case.name
            ),
            None => assert!(actual.is_err(), "{}", case.name),
        }
        assert_state(
            &proof_root,
            expected_state(&case.after, &oracle),
            &case.name,
        );
    }

    // execute every read-only owner identity or SQL projection against exact retained state
    for case in &oracle.owner_queries {
        let evidence = directory.join(format!("owner-query-{}", case.name));
        fs::create_dir(&evidence).unwrap();
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700)).unwrap();
        let before = [
            case.before[0].clone(),
            case.before[1].clone(),
            "-".to_owned(),
            "-".to_owned(),
            "-".to_owned(),
        ];
        let proof_root = install_state(&evidence, &before, &oracle);
        let mut files =
            NativeShadowMetadataCustodyFiles::for_evidence_root(evidence, owner.uid(), owner.gid());
        let mut codec = weather_runtime::NativeWirePrimitives;
        let actual = shadow_metadata_custody_owner_command(
            &case.action,
            &[case.argument.as_str()],
            &mut Cursor::new(&[]),
            &mut files,
            &mut codec,
            &time,
            oracle.clocks.keys().next().unwrap(),
        );
        // compare exact raw identity or SQL bytes and equivalent original refusal
        match &case.expected {
            Some(expected) => assert_eq!(
                actual.expect(&case.name).as_deref(),
                Some(expected.as_slice()),
                "{}",
                case.name
            ),
            None => assert!(actual.is_err(), "{}", case.name),
        }
        assert_state(&proof_root, expected_state(&before, &oracle), &case.name);
    }

    // execute owner mutations from the same noncanonical bytes accepted by the Node bridge
    for case in &oracle.owner_mutations {
        let evidence = directory.join(format!("owner-mutation-{}", case.name));
        fs::create_dir(&evidence).unwrap();
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700)).unwrap();
        let proof_root = install_state(&evidence, &case.before, &oracle);
        let mut files =
            NativeShadowMetadataCustodyFiles::for_evidence_root(evidence, owner.uid(), owner.gid());
        let mut codec = weather_runtime::NativeWirePrimitives;
        let mut input = Cursor::new(oracle.raw_documents[&case.input].bytes.as_slice());
        let actual = shadow_metadata_custody_owner_command(
            &case.action,
            &[case.argument.as_str()],
            &mut input,
            &mut files,
            &mut codec,
            &time,
            &case.now,
        );
        // compare canonical terminal documents while retaining ordinary owner input whitespace
        match &case.expected {
            Some(expected) => assert_eq!(
                actual.expect(&case.name).as_deref(),
                Some(oracle.documents[expected].bytes.as_slice()),
                "{}",
                case.name
            ),
            None => assert!(actual.is_err(), "{}", case.name),
        }
        assert_state(
            &proof_root,
            expected_state(&case.after, &oracle),
            &case.name,
        );
    }
}

/// Compare native proof prepare, publish, and retry bytes with the Node-generated proof fixture.
#[test]
fn shadow_custody_proof_writer_matches_node_bytes_and_retries() {
    let oracle = Oracle::load();
    let proof_document = &oracle.documents["proofTwo"];
    let proof = fixture_node(proof_document, ".");
    let mut input = publish_input(&proof);
    input.entries.reverse();
    let acknowledgement = input.acknowledgement_sha256.clone();
    let root = std::env::temp_dir().join(format!(
        "weather-shadow-custody-writer-parity-{}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("unique writer fixture root");
    let fixture = FixtureRoot(root);
    let directory = fs::canonicalize(&fixture.0).unwrap();
    let owner = fs::metadata(&directory).unwrap();
    let evidence = directory.join("proof");
    fs::create_dir(&evidence).unwrap();
    fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700)).unwrap();
    let proof_root = evidence.join("shadow-metadata-custody-proofs");
    let mut files =
        NativeShadowMetadataCustodyFiles::for_evidence_root(evidence, owner.uid(), owner.gid());
    let mut codec = Codec(&oracle);
    let time = Time(&oracle);

    let prepared =
        prepare_shadow_metadata_custody_proof(input.clone(), &mut files, &mut codec, &time)
            .unwrap();
    assert_eq!(prepared.as_ref(), Some(&proof));
    assert_eq!(
        read_files(&proof_root),
        BTreeMap::from([("pending.json".to_owned(), proof_document.bytes.clone())])
    );

    let prepared_retry =
        prepare_shadow_metadata_custody_proof(input.clone(), &mut files, &mut codec, &time)
            .unwrap();
    assert_eq!(prepared_retry, prepared);
    assert_eq!(
        read_files(&proof_root),
        BTreeMap::from([("pending.json".to_owned(), proof_document.bytes.clone())])
    );

    let committed =
        commit_shadow_metadata_custody_proof(&acknowledgement, &mut files, &mut codec, &time)
            .unwrap();
    assert_eq!(committed.as_ref(), Some(&proof));
    assert_eq!(
        read_files(&proof_root),
        BTreeMap::from([("current.json".to_owned(), proof_document.bytes.clone())])
    );

    let prepared_retry =
        prepare_shadow_metadata_custody_proof(input, &mut files, &mut codec, &time).unwrap();
    assert_eq!(prepared_retry, prepared);
    let committed_retry =
        commit_shadow_metadata_custody_proof(&acknowledgement, &mut files, &mut codec, &time)
            .unwrap();
    assert_eq!(committed_retry, committed);
    assert_eq!(
        read_files(&proof_root),
        BTreeMap::from([("current.json".to_owned(), proof_document.bytes.clone())])
    );

    let empty_evidence = directory.join("no-shadow");
    fs::create_dir(&empty_evidence).unwrap();
    fs::set_permissions(&empty_evidence, fs::Permissions::from_mode(0o700)).unwrap();
    let empty_proof_root = empty_evidence.join("shadow-metadata-custody-proofs");
    let mut empty_files = NativeShadowMetadataCustodyFiles::for_evidence_root(
        empty_evidence,
        owner.uid(),
        owner.gid(),
    );
    let mut empty_input = publish_input(&proof);
    empty_input.entries.clear();
    assert_eq!(
        prepare_shadow_metadata_custody_proof(empty_input, &mut empty_files, &mut codec, &time,)
            .unwrap(),
        None
    );
    assert_state(
        &empty_proof_root,
        BTreeMap::new(),
        "no-shadow proof preparation",
    );
}
