use std::cell::{Cell, RefCell};
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
    FamilyCatalogImageVerification, FamilyReleaseBoundImageRead, FamilyReleaseEnvironments,
    FamilyReleaseImagePorts, FamilyTargetImageRead, FamilyTargetPreflightInput,
    FamilyTargetWitnessRead, MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson,
    MaintenanceCanonicalJson, NativeFamilyActionAuthorityFiles, NativeFamilyTargetWitness,
    prepare_family_release_material, prepare_family_target_catalog,
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
    witness: Option<String>,
    anchor: String,
    anchor_kind: String,
    expected: Option<String>,
    clock_calls: usize,
    witness_reads: usize,
}

struct Material {
    target_environment: String,
    compensation_environment: String,
    source_registry: String,
    target_registry: String,
    compensation_registry: String,
    authority: String,
    target_family_proof: String,
    target_family_sha256: String,
    compensation_family_proof: String,
    compensation_family_sha256: String,
    target_image_proof: String,
    target_image_sha256: String,
    compensation_image_proof: String,
    compensation_image_sha256: String,
}

#[derive(Clone)]
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
    materials: BTreeMap<String, Material>,
    members: BTreeMap<String, Vec<Member>>,
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
    // invoke the composed original Node authorities and retain only their closed fixture protocol
    fn load() -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/family-target-preflight-node-oracle.mjs");
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
            .expect("Node family target preflight oracle");
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        let stdout = String::from_utf8(child.stdout).unwrap();
        let mut oracle = Self::default();
        // decode every source document, digest, clock, read and composed outcome exactly once
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
                    witness,
                    anchor,
                    anchor_kind,
                    expected,
                    clock_calls,
                    witness_reads,
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
                    witness: (*witness != "-").then(|| (*witness).to_owned()),
                    anchor: (*anchor).to_owned(),
                    anchor_kind: (*anchor_kind).to_owned(),
                    expected: (*expected != "error").then(|| (*expected).to_owned()),
                    clock_calls: clock_calls.parse().unwrap(),
                    witness_reads: witness_reads.parse().unwrap(),
                }),
                [
                    "material",
                    name,
                    target_environment,
                    compensation_environment,
                    source_registry,
                    target_registry,
                    compensation_registry,
                    authority,
                    target_family_proof,
                    target_family_sha256,
                    compensation_family_proof,
                    compensation_family_sha256,
                    target_image_proof,
                    target_image_sha256,
                    compensation_image_proof,
                    compensation_image_sha256,
                ] => {
                    oracle.materials.insert(
                        (*name).to_owned(),
                        Material {
                            target_environment: (*target_environment).to_owned(),
                            compensation_environment: (*compensation_environment).to_owned(),
                            source_registry: (*source_registry).to_owned(),
                            target_registry: (*target_registry).to_owned(),
                            compensation_registry: (*compensation_registry).to_owned(),
                            authority: (*authority).to_owned(),
                            target_family_proof: (*target_family_proof).to_owned(),
                            target_family_sha256: (*target_family_sha256).to_owned(),
                            compensation_family_proof: (*compensation_family_proof).to_owned(),
                            compensation_family_sha256: (*compensation_family_sha256).to_owned(),
                            target_image_proof: (*target_image_proof).to_owned(),
                            target_image_sha256: (*target_image_sha256).to_owned(),
                            compensation_image_proof: (*compensation_image_proof).to_owned(),
                            compensation_image_sha256: (*compensation_image_sha256).to_owned(),
                        },
                    );
                }
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
                _ => panic!("unexpected family target preflight oracle row: {row}"),
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
    events: &'a RefCell<Vec<String>>,
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
        self.events.borrow_mut().push(format!("member:{path}"));
        member
            .document
            .as_ref()
            .map(|name| self.oracle.documents[name].bytes.clone())
            .ok_or(MaintenanceAnchorStatusError::Slot)
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ReleaseEvent {
    Read {
        owner: &'static str,
        path: String,
        maximum: usize,
    },
    Clock,
    Witness,
    Verify {
        image: String,
        bytes: Vec<u8>,
    },
}

struct ReleaseImage<'a> {
    oracle: &'a Oracle,
    owner: &'static str,
    reference: String,
    members: Vec<Member>,
    index: &'a Cell<usize>,
    events: &'a RefCell<Vec<ReleaseEvent>>,
    case: &'a str,
}

impl FamilyTargetImageRead for ReleaseImage<'_> {
    // preserve the selected owner and exact ordered member reads
    fn read_member(
        &mut self,
        path: &str,
        maximum: usize,
    ) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        let member = self
            .members
            .get(self.index.get())
            .unwrap_or_else(|| panic!("{} unexpected {} member {path}", self.case, self.owner));
        assert_eq!(path, member.path, "{}", self.case);
        assert_eq!(maximum, member.maximum, "{}", self.case);
        self.index.set(self.index.get() + 1);
        self.events.borrow_mut().push(ReleaseEvent::Read {
            owner: self.owner,
            path: path.to_owned(),
            maximum,
        });
        member
            .document
            .as_ref()
            .map(|name| self.oracle.documents[name].bytes.clone())
            .ok_or(MaintenanceAnchorStatusError::Slot)
    }
}

impl FamilyReleaseBoundImageRead for ReleaseImage<'_> {
    // name the immutable image selected for this reader
    fn image_reference(&self) -> &str {
        &self.reference
    }
}

struct ReleaseVerifier<'a> {
    events: &'a RefCell<Vec<ReleaseEvent>>,
}

impl FamilyCatalogImageVerification for ReleaseVerifier<'_> {
    // record the exact compiled-loader verification boundary
    fn verify_catalog(
        &mut self,
        image: &str,
        bytes: &[u8],
    ) -> Result<(), MaintenanceAnchorStatusError> {
        self.events.borrow_mut().push(ReleaseEvent::Verify {
            image: image.to_owned(),
            bytes: bytes.to_vec(),
        });
        Ok(())
    }
}

struct Witness<'a> {
    inner: NativeFamilyTargetWitness,
    reads: &'a Cell<usize>,
    events: &'a RefCell<Vec<String>>,
}

struct ReleaseWitness<'a> {
    inner: NativeFamilyTargetWitness,
    reads: &'a Cell<usize>,
    events: &'a RefCell<Vec<ReleaseEvent>>,
}

impl FamilyTargetWitnessRead for ReleaseWitness<'_> {
    // record the independently observed witness boundary
    fn read_witness(&mut self) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        self.reads.set(self.reads.get() + 1);
        self.events.borrow_mut().push(ReleaseEvent::Witness);
        self.inner.read_witness()
    }
}

impl FamilyTargetWitnessRead for Witness<'_> {
    // reproduce only the independently selected witness observation and record its exact position
    fn read_witness(&mut self) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        self.reads.set(self.reads.get() + 1);
        self.events.borrow_mut().push("witness".to_owned());
        self.inner.read_witness()
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

// write one fixture-owned exact operator witness file
fn write_witness(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
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

// derive the complete observable image-clock-witness order supplied by the original oracle
fn expected_events(case: &Case, members: &[Member]) -> Vec<String> {
    let mut events = members
        .iter()
        .map(|member| format!("member:{}", member.path))
        .collect::<Vec<_>>();
    events.extend(std::iter::repeat_n("clock".to_owned(), case.clock_calls));
    events.extend(std::iter::repeat_n(
        "witness".to_owned(),
        case.witness_reads,
    ));
    events
}

// obtain one exact selected server-image reference from an oracle document
fn server_image(oracle: &Oracle, document: &str) -> String {
    oracle.documents[document].nodes["$/WEATHER_SERVER_IMAGE"]
        .1
        .clone()
}

// prove the emitted canonical proof document owns the expected Node digest
fn assert_proof_hash(oracle: &Oracle, document: &str, expected: &str) {
    assert_eq!(
        oracle.hashes[oracle.documents[document].bytes.as_slice()],
        expected
    );
}

/// Compare the composed native preflight with the original Node output, refusal and read trace.
#[test]
fn family_target_preflight_matches_node_and_leaves_authority_state_unchanged() {
    let oracle = Oracle::load();
    assert_eq!(oracle.cases.len(), 5);
    assert_eq!(oracle.materials.len(), 1);
    assert_eq!(oracle.documents.len(), 32);
    assert_eq!(
        oracle
            .documents
            .values()
            .map(|document| document.nodes.len())
            .sum::<usize>(),
        482
    );
    assert_eq!(oracle.hashes.len(), 106);
    assert_eq!(oracle.clocks.len(), 7);
    assert_eq!(oracle.members.values().map(Vec::len).sum::<usize>(), 6);
    let root = std::env::temp_dir().join(format!(
        "weather-family-target-preflight-parity-{}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("unique fixture root");
    let fixture = FixtureRoot(root);
    let directory = fs::canonicalize(&fixture.0).unwrap();
    let owner = fs::metadata(&directory).unwrap();
    let time = Time(&oracle);

    // exercise every composed original outcome through isolated native authority files
    for case in &oracle.cases {
        let evidence = directory.join(&case.name);
        fs::create_dir(&evidence).unwrap();
        fs::set_permissions(&evidence, fs::Permissions::from_mode(0o700)).unwrap();
        let anchor_root = evidence.join(anchor_directory(&case.anchor_kind));
        fs::create_dir(&anchor_root).unwrap();
        fs::set_permissions(&anchor_root, fs::Permissions::from_mode(0o700)).unwrap();
        write_current(
            &anchor_root.join("current.json"),
            &oracle.documents[&case.anchor].bytes,
        );
        let witness_path = evidence.join("adjustment-capture-epoch.json");
        // install a witness only for cases whose original composition can observe it
        if let Some(witness) = &case.witness {
            write_witness(&witness_path, &oracle.documents[witness].bytes);
        }
        let before = snapshot(&evidence);
        let action = fixture_node(&oracle.documents[&case.action], "$");
        let environment = fixture_node(&oracle.documents[&case.environment], "$");
        let members: &[Member] = oracle.members.get(&case.name).map_or(&[], Vec::as_slice);
        let image_reads = Cell::new(0);
        let clock_calls = Cell::new(0);
        let witness_reads = Cell::new(0);
        let events = RefCell::new(Vec::new());
        // sample the builder clock only after the exact original image member sequence
        let clock = || {
            assert_eq!(image_reads.get(), members.len(), "{}", case.name);
            clock_calls.set(clock_calls.get() + 1);
            events.borrow_mut().push("clock".to_owned());
            Ok(case.installed_at.clone())
        };
        let input = FamilyTargetPreflightInput {
            action: &action,
            action_sha256: &case.action_sha256,
            family: &case.family,
            source_bytes: &oracle.documents[&case.source].bytes,
            environment: &environment,
            image_commit: &case.image_commit,
            settings_bytes: &oracle.documents[&case.settings].bytes,
            clock: &clock,
        };
        let mut files = NativeFamilyActionAuthorityFiles::for_evidence_root(
            evidence.clone(),
            owner.uid(),
            owner.gid(),
            owner.uid(),
        );
        let mut image = Image {
            oracle: &oracle,
            members,
            index: &image_reads,
            events: &events,
            case: &case.name,
        };
        let mut witness = Witness {
            inner: NativeFamilyTargetWitness::for_path(
                witness_path.clone(),
                owner.uid(),
                owner.gid(),
            ),
            reads: &witness_reads,
            events: &events,
        };
        let mut codec = Codec(&oracle);
        let actual = prepare_family_target_catalog(
            &input,
            &mut files,
            &mut image,
            &mut witness,
            &mut codec,
            &time,
        );
        // compare exact canonical output or the equivalent original composed refusal
        match &case.expected {
            Some(expected) => assert_eq!(
                actual.expect(&case.name).as_slice(),
                oracle.documents[expected].bytes.as_slice(),
                "{}",
                case.name
            ),
            None => assert!(actual.is_err(), "{}", case.name),
        }
        assert_eq!(image_reads.get(), members.len(), "{}", case.name);
        assert_eq!(clock_calls.get(), case.clock_calls, "{}", case.name);
        assert_eq!(witness_reads.get(), case.witness_reads, "{}", case.name);
        assert_eq!(
            *events.borrow(),
            expected_events(case, members),
            "{}",
            case.name
        );
        assert_eq!(snapshot(&evidence), before, "{}", case.name);

        // exercise the additive complete release-material composition only for its oracle row
        if let Some(material) = oracle.materials.get(&case.name) {
            let target_environment =
                fixture_node(&oracle.documents[&material.target_environment], "$");
            let compensation_environment =
                fixture_node(&oracle.documents[&material.compensation_environment], "$");
            let target_image = server_image(&oracle, &material.target_environment);
            let compensation_image = server_image(&oracle, &material.compensation_environment);
            let registry_path = "config/forecast-adjustments/ballydidean-temperature-canary.json";
            let registry_maximum = 8 * 1024 * 1024;
            let source_reads = Cell::new(0);
            let target_reads = Cell::new(0);
            let compensation_reads = Cell::new(0);
            let release_clock_calls = Cell::new(0);
            let release_witness_reads = Cell::new(0);
            let release_events = RefCell::new(Vec::new());
            let mut target_members = members.to_vec();
            target_members.push(Member {
                path: registry_path.to_owned(),
                maximum: registry_maximum,
                document: Some(material.target_registry.clone()),
            });
            let mut source_image = ReleaseImage {
                oracle: &oracle,
                owner: "source",
                reference: compensation_image.clone(),
                members: vec![Member {
                    path: registry_path.to_owned(),
                    maximum: registry_maximum,
                    document: Some(material.source_registry.clone()),
                }],
                index: &source_reads,
                events: &release_events,
                case: &case.name,
            };
            let mut target_image_reader = ReleaseImage {
                oracle: &oracle,
                owner: "target",
                reference: target_image.clone(),
                members: target_members,
                index: &target_reads,
                events: &release_events,
                case: &case.name,
            };
            let mut compensation_image_reader = ReleaseImage {
                oracle: &oracle,
                owner: "compensation",
                reference: compensation_image.clone(),
                members: vec![Member {
                    path: registry_path.to_owned(),
                    maximum: registry_maximum,
                    document: Some(material.compensation_registry.clone()),
                }],
                index: &compensation_reads,
                events: &release_events,
                case: &case.name,
            };
            let mut verifier = ReleaseVerifier {
                events: &release_events,
            };
            // sample the builder clock before any release registry or loader observation
            let release_clock = || {
                assert_eq!(target_reads.get(), members.len(), "{}", case.name);
                assert_eq!(source_reads.get(), 0, "{}", case.name);
                assert_eq!(compensation_reads.get(), 0, "{}", case.name);
                release_clock_calls.set(release_clock_calls.get() + 1);
                release_events.borrow_mut().push(ReleaseEvent::Clock);
                Ok(case.installed_at.clone())
            };
            let release_input = FamilyTargetPreflightInput {
                action: &action,
                action_sha256: &case.action_sha256,
                family: &case.family,
                source_bytes: &oracle.documents[&case.source].bytes,
                environment: &target_environment,
                image_commit: &case.image_commit,
                settings_bytes: &oracle.documents[&case.settings].bytes,
                clock: &release_clock,
            };
            let mut release_files = NativeFamilyActionAuthorityFiles::for_evidence_root(
                evidence.clone(),
                owner.uid(),
                owner.gid(),
                owner.uid(),
            );
            let mut release_witness = ReleaseWitness {
                inner: NativeFamilyTargetWitness::for_path(
                    witness_path.clone(),
                    owner.uid(),
                    owner.gid(),
                ),
                reads: &release_witness_reads,
                events: &release_events,
            };
            let mut release_codec = Codec(&oracle);
            let actual = prepare_family_release_material(
                &release_input,
                &FamilyReleaseEnvironments {
                    source: &compensation_environment,
                    compensation: &compensation_environment,
                },
                &mut release_files,
                &mut FamilyReleaseImagePorts {
                    source: &mut source_image,
                    target: &mut target_image_reader,
                    compensation: &mut compensation_image_reader,
                    verifier: &mut verifier,
                },
                &mut release_witness,
                &mut release_codec,
                &time,
            )
            .expect(&case.name);
            let expected_catalog = &oracle.documents[case.expected.as_ref().unwrap()].bytes;
            assert_eq!(
                actual.authority,
                fixture_node(&oracle.documents[&material.authority], "$"),
                "{}",
                case.name
            );
            assert_eq!(
                actual.target_catalog_bytes, *expected_catalog,
                "{}",
                case.name
            );
            assert_eq!(
                actual.identities.target_family.as_str(),
                material.target_family_sha256
            );
            assert_eq!(
                actual.identities.compensation_family.as_str(),
                material.compensation_family_sha256
            );
            assert_eq!(
                actual.identities.target_images.as_str(),
                material.target_image_sha256
            );
            assert_eq!(
                actual.identities.compensation_images.as_str(),
                material.compensation_image_sha256
            );
            assert_proof_hash(
                &oracle,
                &material.target_family_proof,
                &material.target_family_sha256,
            );
            assert_proof_hash(
                &oracle,
                &material.compensation_family_proof,
                &material.compensation_family_sha256,
            );
            assert_proof_hash(
                &oracle,
                &material.target_image_proof,
                &material.target_image_sha256,
            );
            assert_proof_hash(
                &oracle,
                &material.compensation_image_proof,
                &material.compensation_image_sha256,
            );
            assert_eq!(source_reads.get(), 1, "{}", case.name);
            assert_eq!(target_reads.get(), members.len() + 1, "{}", case.name);
            assert_eq!(compensation_reads.get(), 1, "{}", case.name);
            assert_eq!(release_clock_calls.get(), 1, "{}", case.name);
            assert_eq!(release_witness_reads.get(), 1, "{}", case.name);
            let expected_release_events = vec![
                ReleaseEvent::Read {
                    owner: "target",
                    path: members[0].path.clone(),
                    maximum: members[0].maximum,
                },
                ReleaseEvent::Clock,
                ReleaseEvent::Witness,
                ReleaseEvent::Verify {
                    image: target_image,
                    bytes: expected_catalog.to_vec(),
                },
                ReleaseEvent::Verify {
                    image: compensation_image,
                    bytes: oracle.documents[&case.source].bytes.clone(),
                },
                ReleaseEvent::Read {
                    owner: "source",
                    path: registry_path.to_owned(),
                    maximum: registry_maximum,
                },
                ReleaseEvent::Read {
                    owner: "target",
                    path: registry_path.to_owned(),
                    maximum: registry_maximum,
                },
                ReleaseEvent::Read {
                    owner: "compensation",
                    path: registry_path.to_owned(),
                    maximum: registry_maximum,
                },
            ];
            assert_eq!(
                *release_events.borrow(),
                expected_release_events,
                "{}",
                case.name
            );
            assert_eq!(snapshot(&evidence), before, "{}", case.name);
        }
    }
}
