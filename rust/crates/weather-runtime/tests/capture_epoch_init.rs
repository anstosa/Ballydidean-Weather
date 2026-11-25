//! Compare Rust capture-epoch initialization with the direct unprivileged Node writer.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_runtime::{
    CaptureEpochInitDeployment, CaptureEpochInitError, CaptureEpochInitJson, CaptureEpochInitPort,
    CaptureEpochOperatorFile, MaintenanceCanonicalJson, initialize_capture_epoch_operator_command,
};

#[derive(Default)]
struct Material {
    bytes: Vec<u8>,
    fields: BTreeMap<String, JsonValue>,
    preimage: Vec<u8>,
}

#[derive(Default)]
struct Database {
    bytes: Vec<u8>,
    history: String,
    epoch: String,
    payload: String,
    migrations: Vec<(String, String)>,
}

#[derive(Default)]
struct Oracle {
    material: BTreeMap<String, Material>,
    databases: BTreeMap<String, Database>,
    deployments: BTreeMap<String, BTreeMap<String, String>>,
    hashes: Vec<(Vec<u8>, String)>,
    outcomes: BTreeMap<String, Option<Vec<u8>>>,
}

/// Decode exact bytes exported by Node without adding a JSON or hashing dependency.
fn decode_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    (0..hex.len())
        .step_by(2)
        // decode each exported byte independently
        .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).expect("hex byte"))
        .collect()
}

/// Restore one UTF-16 JSON string scalar.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build a JSON object while preserving the supplied field order.
fn object(fields: impl IntoIterator<Item = (&'static str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            // retain exact property spelling in the dependency-free value
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Reconstruct an object whose scalar decoding was performed by the Node oracle.
fn material_object(material: &Material, omit: Option<&str>) -> JsonValue {
    JsonValue::Object(
        material
            .fields
            .iter()
            // remove only the witness self-address for its unsigned preimage
            .filter(|(key, _)| omit != Some(key.as_str()))
            .map(|(key, value)| (key.encode_utf16().collect(), value.clone()))
            .collect(),
    )
}

impl Oracle {
    /// Execute only the direct Node writer on disposable current-user paths.
    fn load() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root");
        let output = Command::new("node")
            .current_dir(workspace)
            .arg("rust/crates/weather-runtime/tests/capture-epoch-init-node-oracle.mjs")
            .output()
            .expect("run Node capture epoch oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("oracle TSV");
        let mut oracle = Self::default();
        // recover typed values without parsing production JSON in Rust
        for line in stdout.lines() {
            let columns = line.split('\t').collect::<Vec<_>>();
            match columns.as_slice() {
                ["material", name, bytes] => {
                    oracle.material.entry((*name).into()).or_default().bytes = decode_hex(bytes);
                }
                ["field", name, key, "string", value] => {
                    oracle
                        .material
                        .entry((*name).into())
                        .or_default()
                        .fields
                        .insert((*key).into(), string(value));
                }
                ["field", name, key, "number", value] => {
                    oracle
                        .material
                        .entry((*name).into())
                        .or_default()
                        .fields
                        .insert(
                            (*key).into(),
                            JsonValue::Number(value.parse().expect("number")),
                        );
                }
                ["field", name, key, "array-empty", ""] => {
                    oracle
                        .material
                        .entry((*name).into())
                        .or_default()
                        .fields
                        .insert((*key).into(), JsonValue::Array(Vec::new()));
                }
                ["hash", name, preimage, digest] => {
                    let preimage = decode_hex(preimage);
                    oracle.hashes.push((preimage.clone(), (*digest).into()));
                    // retain unsigned document bytes beside their decoded object
                    if let Some(material) = oracle.material.get_mut(*name) {
                        material.preimage = preimage;
                    }
                }
                ["database", name, bytes] => {
                    oracle.databases.entry((*name).into()).or_default().bytes = decode_hex(bytes);
                }
                ["database-meta", name, history, epoch, payload] => {
                    let database = oracle.databases.entry((*name).into()).or_default();
                    database.history = (*history).into();
                    database.epoch = (*epoch).into();
                    database.payload = (*payload).into();
                }
                ["migration", name, index, migration_name, checksum] => {
                    let database = oracle.databases.entry((*name).into()).or_default();
                    assert_eq!(
                        index.parse::<usize>().expect("migration index"),
                        database.migrations.len()
                    );
                    database
                        .migrations
                        .push(((*migration_name).into(), (*checksum).into()));
                }
                ["deployment", name, key, value] => {
                    oracle
                        .deployments
                        .entry((*name).into())
                        .or_default()
                        .insert((*key).into(), (*value).into());
                }
                ["outcome", name, "ok", bytes, ""] => {
                    oracle
                        .outcomes
                        .insert((*name).into(), Some(decode_hex(bytes)));
                }
                ["outcome", name, "error", "", _] => {
                    oracle.outcomes.insert((*name).into(), None);
                }
                _ => panic!("unexpected oracle row: {line}"),
            }
        }
        oracle
    }

    /// Reconstruct the exact database object already validated by the Node fixture.
    fn database_value(&self, name: &str) -> JsonValue {
        // preserve the deliberately missing object for closed-key validation
        if name == "missing_envelope" {
            return JsonValue::Object(Vec::new());
        }
        let database = &self.databases[name];
        let migration_names = JsonValue::Array(
            database
                .migrations
                .iter()
                // retain actual checked-in names in database order
                .map(|(name, _)| string(name))
                .collect(),
        );
        let migration_checksums = JsonValue::Array(
            database
                .migrations
                .iter()
                // retain actual checked-in checksums in database order
                .map(|(_, checksum)| string(checksum))
                .collect(),
        );
        let manifest = object([
            (
                "contract_version",
                string("adjustment-evaluation-export-manifest/v1"),
            ),
            ("migration_checksums", migration_checksums),
            ("migration_history_sha256", string(&database.history)),
            ("migration_names", migration_names),
            ("query_contract_sha256", string(&"1".repeat(64))),
            (
                "query_contract_version",
                string("adjustment-evaluation-export-query/v1"),
            ),
            ("row_schema_sha256", string(&"2".repeat(64))),
            (
                "schema_migration",
                string("0017_adjustment_evaluation_export.sql"),
            ),
            ("site_key", string("ballydidean")),
            ("site_timezone", string("America/Los_Angeles")),
        ]);
        let payload = match database.payload.as_str() {
            "null" => JsonValue::Null,
            "snapshot" if name == "direct_a" => {
                material_object(&self.material["direct_a_snapshot"], None)
            }
            "snapshot" if name == "direct_b" => {
                material_object(&self.material["direct_b_snapshot"], None)
            }
            "snapshot" => material_object(&self.material["rolling_snapshot"], None),
            other => panic!("unexpected payload marker: {other}"),
        };
        let transaction = object([
            ("created_at_utc", string(&database.epoch)),
            ("idle_in_transaction_session_timeout", string("30s")),
            ("isolation_level", string("repeatable read")),
            ("lock_timeout", string("5s")),
            ("read_only", string("on")),
            ("statement_timeout", string("5min")),
        ]);
        object([
            ("databaseManifest", manifest),
            ("payload", payload),
            ("transaction", transaction),
        ])
    }

    /// Recover one public deployment with no environment or host interaction.
    fn deployment(&self, name: &str) -> CaptureEpochInitDeployment {
        let fields = &self.deployments[name];
        CaptureEpochInitDeployment {
            control_plane_sha256: fields["controlPlaneSha256"].clone(),
            control_plane_version: fields["controlPlaneVersion"].clone(),
            source_commit: fields["sourceCommit"].clone(),
            source_release: fields["sourceRelease"].clone(),
            source_server_image_digest: fields["sourceServerImageDigest"].clone(),
            source_web_image_digest: fields["sourceWebImageDigest"].clone(),
        }
    }
}

struct Codec<'a>(&'a Oracle);

impl MaintenanceCanonicalJson for Codec<'_> {
    /// Decode only exact canonical documents emitted by the original writer.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.0
            .material
            .values()
            // require exact original bytes before returning their decoded value
            .find(|material| material.bytes == bytes)
            .map(|material| material_object(material, None))
    }

    /// Return only actual canonical documents or their actual unsigned witness bytes.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        // compare every Node-emitted document without serializing it in Rust
        for material in self.0.material.values() {
            // match one complete snapshot or witness
            if value == &material_object(material, None) {
                return Some(material.bytes.clone());
            }
            // match only a witness with its self-address removed
            if material.fields.contains_key("witnessSha256")
                && value == &material_object(material, Some("witnessSha256"))
            {
                return Some(material.preimage.clone());
            }
        }
        None
    }

    /// Bind every requested digest to an exact Node-exported preimage.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.0
            .hashes
            .iter()
            // reject any preimage not observed in the Node oracle
            .find(|(preimage, _)| preimage == bytes)
            .map(|(_, digest)| Sha256Digest::admitted(digest.clone()))
    }
}

impl CaptureEpochInitJson for Codec<'_> {
    /// Decode only exact database stdin bytes emitted by Node.
    fn parse_database_envelope(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.0
            .databases
            .iter()
            // bind the decoded object to exact input bytes
            .find(|(_, database)| database.bytes == bytes)
            .map(|(name, _)| self.0.database_value(name))
    }
}

impl CurrentTimeResolver for Oracle {
    /// Echo only the canonical clock text after the Rust grammar admits it.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let JsDateOrString::String(value) = value else {
            return Err(CurrentProjectionError {
                code: "fixture.clock",
                detail: "expected string clock".to_owned(),
            });
        };
        let utc = String::from_utf16(value).map_err(|_| CurrentProjectionError {
            code: "fixture.clock",
            detail: "invalid fixture UTF-16".to_owned(),
        })?;
        Ok(CanonicalInstant {
            utc,
            epoch_milliseconds: 0,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PortCall {
    Root,
    Resolve(String),
    ColdArchive,
    Read(CaptureEpochOperatorFile),
    Create(CaptureEpochOperatorFile),
}

struct Port {
    deployment: CaptureEpochInitDeployment,
    witness: Option<Vec<u8>>,
    snapshot: Option<Vec<u8>>,
    injected_witness: Option<Vec<u8>>,
    calls: Vec<PortCall>,
}

impl Port {
    /// Construct one in-memory create-once boundary with no host privileges.
    fn new(deployment: CaptureEpochInitDeployment) -> Self {
        Self {
            deployment,
            witness: None,
            snapshot: None,
            injected_witness: None,
            calls: Vec::new(),
        }
    }
}

impl CaptureEpochInitPort for Port {
    /// Record the root gate without acquiring any host privilege.
    fn require_root(&mut self) -> Result<(), CaptureEpochInitError> {
        self.calls.push(PortCall::Root);
        Ok(())
    }

    /// Return the fixture's already admitted immutable deployment.
    fn resolve_deployment(
        &mut self,
        target: &str,
    ) -> Result<CaptureEpochInitDeployment, CaptureEpochInitError> {
        self.calls.push(PortCall::Resolve(target.to_owned()));
        Ok(self.deployment.clone())
    }

    /// Record cold archive preparation without touching a live archive.
    fn initialize_cold_archive(&mut self) -> Result<(), CaptureEpochInitError> {
        self.calls.push(PortCall::ColdArchive);
        Ok(())
    }

    /// Return only disposable fixture bytes.
    fn read_optional(
        &mut self,
        artifact: CaptureEpochOperatorFile,
    ) -> Result<Option<Vec<u8>>, CaptureEpochInitError> {
        self.calls.push(PortCall::Read(artifact));
        Ok(match artifact {
            CaptureEpochOperatorFile::Witness => self.witness.clone(),
            CaptureEpochOperatorFile::Snapshot => self.snapshot.clone(),
        })
    }

    /// Model exclusive create and an optional racing witness winner.
    fn create_once(
        &mut self,
        artifact: CaptureEpochOperatorFile,
        bytes: &[u8],
    ) -> Result<(), CaptureEpochInitError> {
        self.calls.push(PortCall::Create(artifact));
        match artifact {
            CaptureEpochOperatorFile::Witness => {
                // retain an injected winner to exercise collision handling
                if self.witness.is_none() {
                    self.witness = Some(
                        self.injected_witness
                            .take()
                            .unwrap_or_else(|| bytes.to_vec()),
                    );
                }
            }
            CaptureEpochOperatorFile::Snapshot => {
                // preserve the first snapshot exactly
                if self.snapshot.is_none() {
                    self.snapshot = Some(bytes.to_vec());
                }
            }
        }
        Ok(())
    }
}

/// Invoke one fixture database through the same bounded reader used by the command.
fn initialize(
    name: &str,
    target: &str,
    oracle: &Oracle,
    port: &mut Port,
    codec: &mut Codec<'_>,
) -> Result<Option<Vec<u8>>, CaptureEpochInitError> {
    initialize_capture_epoch_operator_command(
        "revision-capture-epoch-init-v1",
        &[target],
        oracle.databases[name].bytes.as_slice(),
        port,
        codec,
        oracle,
    )
}

/// Compare first write, retry, interruption and ordinary failures with Node.
#[test]
fn matches_rolling_node_initialization_and_create_once_recovery() {
    use CaptureEpochInitError as Error;
    use CaptureEpochOperatorFile::{Snapshot, Witness};

    let oracle = Oracle::load();
    let target = "2026.10.08-1";
    let expected = oracle.outcomes["rolling_first"]
        .clone()
        .expect("Node first witness");
    assert_eq!(expected, oracle.material["rolling_witness"].bytes);
    assert_eq!(oracle.outcomes["rolling_retry"], Some(expected.clone()));
    assert_eq!(
        oracle.outcomes["rolling_interrupted_retry"],
        Some(expected.clone())
    );

    let mut codec = Codec(&oracle);
    let mut port = Port::new(oracle.deployment("rolling"));
    assert_eq!(
        initialize("rolling_first", target, &oracle, &mut port, &mut codec),
        Ok(Some(expected.clone()))
    );
    assert_eq!(
        port.snapshot,
        Some(oracle.material["rolling_snapshot"].bytes.clone())
    );
    assert_eq!(port.witness, Some(expected.clone()));
    assert_eq!(
        port.calls,
        [
            PortCall::Root,
            PortCall::Resolve(target.into()),
            PortCall::ColdArchive,
            PortCall::Read(Witness),
            PortCall::Read(Snapshot),
            PortCall::Create(Snapshot),
            PortCall::Read(Snapshot),
            PortCall::Create(Witness),
            PortCall::Read(Witness),
        ]
    );

    port.calls.clear();
    assert_eq!(
        initialize("rolling_retry", target, &oracle, &mut port, &mut codec),
        Ok(Some(expected.clone()))
    );
    assert_eq!(
        port.calls,
        [
            PortCall::Root,
            PortCall::Resolve(target.into()),
            PortCall::ColdArchive,
            PortCall::Read(Witness),
            PortCall::Read(Snapshot),
        ]
    );

    let mut interrupted = Port::new(oracle.deployment("rolling"));
    interrupted.snapshot = Some(oracle.material["rolling_snapshot"].bytes.clone());
    assert_eq!(
        initialize(
            "rolling_retry",
            target,
            &oracle,
            &mut interrupted,
            &mut codec,
        ),
        Ok(Some(expected.clone()))
    );
    assert_eq!(interrupted.witness, Some(expected.clone()));
    assert_eq!(
        interrupted.calls,
        [
            PortCall::Root,
            PortCall::Resolve(target.into()),
            PortCall::ColdArchive,
            PortCall::Read(Witness),
            PortCall::Read(Snapshot),
            PortCall::Create(Witness),
            PortCall::Read(Witness),
        ]
    );

    assert_eq!(oracle.outcomes["rolling_missing_snapshot"], None);
    let mut missing = Port::new(oracle.deployment("rolling"));
    assert_eq!(
        initialize("rolling_retry", target, &oracle, &mut missing, &mut codec),
        Err(Error::MissingSnapshot)
    );

    assert_eq!(oracle.outcomes["rolling_mismatch"], None);
    let mut mismatch = Port::new(oracle.deployment("mismatch"));
    mismatch.snapshot = Some(oracle.material["rolling_snapshot"].bytes.clone());
    mismatch.witness = Some(expected.clone());
    assert_eq!(
        initialize("rolling_retry", target, &oracle, &mut mismatch, &mut codec,),
        Err(Error::Mismatch)
    );

    assert_eq!(oracle.outcomes["stale_envelope"], None);
    let mut stale = Port::new(oracle.deployment("rolling"));
    assert_eq!(
        initialize("stale_envelope", target, &oracle, &mut stale, &mut codec),
        Err(Error::Ledger)
    );
    assert_eq!(stale.calls, [PortCall::Root]);

    assert_eq!(oracle.outcomes["missing_envelope"], None);
    let mut absent = Port::new(oracle.deployment("rolling"));
    assert_eq!(
        initialize("missing_envelope", target, &oracle, &mut absent, &mut codec,),
        Err(Error::DatabaseInput)
    );
    assert_eq!(absent.calls, [PortCall::Root]);
}

/// Admit both reviewed direct ledgers and detect a racing different witness.
#[test]
fn matches_both_direct_node_ledgers_and_rejects_collision() {
    use CaptureEpochInitError::Collision;

    let oracle = Oracle::load();
    let mut codec = Codec(&oracle);
    // compare both known checked checksum variants with their actual Node bytes
    for name in ["direct_a", "direct_b"] {
        let expected = oracle.outcomes[name].clone().expect("Node direct witness");
        assert_eq!(expected, oracle.material[&format!("{name}_witness")].bytes);
        let mut port = Port::new(oracle.deployment("direct"));
        assert_eq!(
            initialize(name, "2026.11.03-1", &oracle, &mut port, &mut codec,),
            Ok(Some(expected.clone()))
        );
        assert_eq!(port.witness, Some(expected));
        assert_eq!(
            port.snapshot,
            Some(oracle.material[&format!("{name}_snapshot")].bytes.clone())
        );
    }

    let mut collision = Port::new(oracle.deployment("rolling"));
    collision.injected_witness = Some(oracle.material["direct_a_witness"].bytes.clone());
    assert_eq!(
        initialize(
            "rolling_first",
            "2026.10.08-1",
            &oracle,
            &mut collision,
            &mut codec,
        ),
        Err(Collision)
    );
}
