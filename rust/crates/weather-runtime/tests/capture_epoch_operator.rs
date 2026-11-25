//! Compare the Rust operator reader with the unchanged Node command on real fixture files.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::{JsDateOrString, JsonValue};
use weather_database::Sha256Digest;
use weather_runtime::{
    CaptureEpochOperatorFile, CaptureEpochOperatorFiles, CaptureEpochOperatorReadError,
    MaintenanceCanonicalJson, read_capture_epoch_operator_command, verify_capture_epoch_witness,
};

#[derive(Default)]
struct Material {
    bytes: Vec<u8>,
    fields: BTreeMap<String, JsonValue>,
    preimage: Vec<u8>,
    digest: String,
}

#[derive(Default)]
struct Oracle {
    material: BTreeMap<String, Material>,
    clocks: BTreeMap<String, i64>,
    outcomes: BTreeMap<String, Option<Vec<u8>>>,
}

/// Restore the oracle's stripped canonical line terminator.
fn canonical_line(text: &str) -> Vec<u8> {
    [text.as_bytes(), b"\n"].concat()
}

/// Decode a fixture byte sequence rather than parsing any contract data in test code.
fn decode_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    (0..hex.len())
        .step_by(2)
        // read each byte exported by the original fixture
        .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).expect("hex fixture byte"))
        .collect()
}

impl Oracle {
    /// Execute the original Node commands and capture the observed file values and outputs.
    fn load() -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root");
        let output = Command::new("node")
            .current_dir(workspace)
            .arg("rust/crates/weather-runtime/tests/capture-epoch-operator-node-oracle.mjs")
            .output()
            .expect("run original Node command fixture");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("oracle TSV");
        let mut oracle = Self::default();
        // recover typed fixture scalars independently of the original stdout bytes
        for line in stdout.lines() {
            match line.split('\t').collect::<Vec<_>>().as_slice() {
                ["material", name, bytes] => {
                    oracle.material.entry((*name).into()).or_default().bytes =
                        canonical_line(bytes);
                }
                ["field", name, key, value] => {
                    oracle
                        .material
                        .entry((*name).into())
                        .or_default()
                        .fields
                        .insert(
                            (*key).into(),
                            JsonValue::String(value.encode_utf16().collect()),
                        );
                }
                ["hash", name, preimage, digest] => {
                    let material = oracle.material.entry((*name).into()).or_default();
                    material.preimage = decode_hex(preimage);
                    material.digest = (*digest).into();
                }
                ["clock", cutoff, milliseconds] => {
                    oracle.clocks.insert(
                        (*cutoff).into(),
                        milliseconds.parse().expect("oracle epoch milliseconds"),
                    );
                }
                ["result", name, "ok", stdout, ""] => {
                    oracle
                        .outcomes
                        .insert((*name).into(), Some(canonical_line(stdout)));
                }
                ["result", name, "error", "", error] => {
                    assert!(!error.is_empty());
                    oracle.outcomes.insert((*name).into(), None);
                }
                _ => panic!("unexpected oracle row: {line}"),
            }
        }
        // retain the decoded empty collection and numeric count of the zero-only snapshots
        for name in [
            "snapshot_a",
            "snapshot_b",
            "snapshot_bad_clock",
            "snapshot_extended",
        ] {
            let fields = &mut oracle.material.get_mut(name).expect("snapshot").fields;
            fields.insert("entries".into(), JsonValue::Array(Vec::new()));
            fields.insert("entryCount".into(), JsonValue::Number(0.0));
        }
        oracle
    }
}

#[derive(Default)]
struct EchoTime(Cell<usize>);

impl CurrentTimeResolver for EchoTime {
    /// Return even a malformed string to test the command's own wire grammar.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        self.0.set(self.0.get() + 1);
        let JsDateOrString::String(utc) = value else {
            panic!("expected a clock spelled as a string");
        };
        Ok(CanonicalInstant {
            utc: String::from_utf16(utc).expect("fixture clock"),
            epoch_milliseconds: 0,
        })
    }
}

/// Build the fixture's already decoded object in the original property order.
fn object(fields: &BTreeMap<String, JsonValue>, omit: Option<&str>) -> JsonValue {
    JsonValue::Object(
        fields
            .iter()
            // exclude only the original self-address for a witness preimage
            .filter(|(key, _)| omit != Some(key.as_str()))
            // preserve the oracle's exact admitted UTF-16 scalars
            .map(|(key, value)| (key.encode_utf16().collect(), value.clone()))
            .collect(),
    )
}

struct Codec<'a>(&'a Oracle);

impl MaintenanceCanonicalJson for Codec<'_> {
    /// Substitute only original byte-to-value cases for the not-yet-selected codec.
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.0
            .material
            .values()
            // find only the exact canonical fixture bytes
            .find(|material| material.bytes == bytes)
            // restore the already decoded object
            .map(|material| object(&material.fields, None))
    }

    /// Return original canonical bytes or the exact unsigned witness preimage.
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        // match the complete decoded value without normalizing scalars
        for material in self.0.material.values() {
            // return the signed document only for an exact matching value
            if value == &object(&material.fields, None) {
                return Some(material.bytes.clone());
            }
            // permit a second encoding only for the witness's original unsigned object
            if material.fields.contains_key("witnessSha256")
                && value == &object(&material.fields, Some("witnessSha256"))
            {
                return Some(material.preimage.clone());
            }
        }
        None
    }

    /// Bind the requested digest to the corresponding original preimage.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.0
            .material
            .values()
            // fail if the reader creates different preimage bytes
            .find(|material| material.preimage == bytes)
            // retain exactly the original SHA-256
            .map(|material| Sha256Digest::admitted(material.digest.clone()))
    }
}

impl CurrentTimeResolver for Oracle {
    /// Reuse original canonical clocks and their Node millisecond values.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        // admit only the original ISO text fixture
        if let JsDateOrString::String(value) = value
            && let Ok(utc) = String::from_utf16(value)
            && let Some(epoch_milliseconds) = self.clocks.get(&utc).copied()
        {
            return Ok(CanonicalInstant {
                utc,
                epoch_milliseconds,
            });
        }
        Err(CurrentProjectionError {
            code: "fixture.clock",
            detail: "clock not exported by the Node oracle".to_owned(),
        })
    }
}

struct Files<'a> {
    witness: Option<&'a [u8]>,
    snapshot: &'a [u8],
    trace: Vec<CaptureEpochOperatorFile>,
}

impl CaptureEpochOperatorFiles for Files<'_> {
    /// Record exact open order while retaining immutable bytes.
    fn read(
        &mut self,
        artifact: CaptureEpochOperatorFile,
    ) -> Result<Vec<u8>, CaptureEpochOperatorReadError> {
        self.trace.push(artifact);
        match artifact {
            CaptureEpochOperatorFile::Witness => self
                .witness
                .map(<[u8]>::to_vec)
                .ok_or(CaptureEpochOperatorReadError::File),
            CaptureEpochOperatorFile::Snapshot => Ok(self.snapshot.to_vec()),
        }
    }
}

/// Compare command stdout and verify the permitted file effects before each output.
#[test]
fn matches_original_epoch_export_and_withholds_detached_witness() {
    use CaptureEpochOperatorFile::{Snapshot, Witness};
    use CaptureEpochOperatorReadError as Error;
    let oracle = Oracle::load();
    let mut codec = Codec(&oracle);
    let mut files = Files {
        witness: None,
        snapshot: &oracle.material["snapshot_a"].bytes,
        trace: Vec::new(),
    };
    let snapshot = "revision-capture-epoch-snapshot-read-v1";
    let witness = "revision-capture-epoch-read-v1";

    assert_eq!(
        read_capture_epoch_operator_command(snapshot, &[], &mut files, &mut codec, &oracle),
        Ok(oracle.outcomes["snapshot_without_witness"].clone())
    );
    assert_eq!(files.trace, [Snapshot]);

    files.trace.clear();
    files.witness = Some(&oracle.material["witness"].bytes);
    assert_eq!(
        read_capture_epoch_operator_command(witness, &[], &mut files, &mut codec, &oracle),
        Ok(oracle.outcomes["paired_witness"].clone())
    );
    assert_eq!(files.trace, [Witness, Snapshot]);

    files.trace.clear();
    files.snapshot = &oracle.material["snapshot_b"].bytes;
    assert_eq!(oracle.outcomes["mismatched_pair"], None);
    assert_eq!(
        read_capture_epoch_operator_command(witness, &[], &mut files, &mut codec, &oracle),
        Err(Error::Mismatch)
    );
    assert_eq!(files.trace, [Witness, Snapshot]);

    files.trace.clear();
    let early = &oracle.material["witness_early_v14"].bytes;
    files.witness = Some(early);
    files.snapshot = &oracle.material["snapshot_a"].bytes;
    assert_eq!(oracle.outcomes["early_v14_witness"], None);
    assert_eq!(
        read_capture_epoch_operator_command(witness, &[], &mut files, &mut codec, &oracle),
        Err(Error::Witness)
    );
    assert_eq!(files.trace, [Witness]);
    assert!(verify_capture_epoch_witness(early, &mut codec, &oracle).is_ok());

    files.trace.clear();
    assert_eq!(oracle.outcomes["snapshot_arity"], None);
    assert_eq!(
        read_capture_epoch_operator_command(snapshot, &["extra"], &mut files, &mut codec, &oracle),
        Err(Error::Arguments)
    );
    assert_eq!(
        read_capture_epoch_operator_command("other-verb", &[], &mut files, &mut codec, &oracle),
        Ok(None)
    );
    assert!(files.trace.is_empty());

    let permissive_time = EchoTime::default();
    files.snapshot = &oracle.material["snapshot_bad_clock"].bytes;
    files.witness = Some(&oracle.material["witness_bad_clock"].bytes);
    assert_eq!(oracle.outcomes["snapshot_bad_clock"], None);
    assert_eq!(
        read_capture_epoch_operator_command(
            snapshot,
            &[],
            &mut files,
            &mut codec,
            &permissive_time
        ),
        Err(Error::Snapshot)
    );
    assert_eq!(files.trace, [Snapshot]);
    files.trace.clear();
    assert_eq!(oracle.outcomes["witness_bad_clock"], None);
    assert_eq!(
        read_capture_epoch_operator_command(witness, &[], &mut files, &mut codec, &permissive_time),
        Err(Error::Witness)
    );
    assert_eq!(files.trace, [Witness]);
    assert_eq!(permissive_time.0.get(), 0);

    files.trace.clear();
    files.snapshot = &oracle.material["snapshot_extended"].bytes;
    files.witness = Some(&oracle.material["witness_extended"].bytes);
    assert_eq!(
        read_capture_epoch_operator_command(snapshot, &[], &mut files, &mut codec, &oracle),
        Ok(oracle.outcomes["snapshot_extended"].clone())
    );
    assert_eq!(
        read_capture_epoch_operator_command(witness, &[], &mut files, &mut codec, &oracle),
        Ok(oracle.outcomes["witness_extended"].clone())
    );
    assert_eq!(files.trace, [Snapshot, Witness, Snapshot]);
}

/// Verify both retained authority read contracts through the selected native wire owner.
#[test]
fn native_capture_epoch_wire_matches_original_authority_exports() {
    let oracle = Oracle::load();
    let mut files = Files {
        witness: Some(&oracle.material["witness"].bytes),
        snapshot: &oracle.material["snapshot_a"].bytes,
        trace: Vec::new(),
    };
    let mut codec = weather_runtime::NativeWirePrimitives;
    let time = weather_runtime::NativeWirePrimitives;
    // retain the exact fixed source authority bytes for each original verb
    for (action, expected) in [
        (
            "revision-capture-epoch-snapshot-read-v1",
            "snapshot_without_witness",
        ),
        ("revision-capture-epoch-read-v1", "paired_witness"),
    ] {
        assert_eq!(
            read_capture_epoch_operator_command(action, &[], &mut files, &mut codec, &time),
            Ok(oracle.outcomes[expected].clone())
        );
    }
    assert_eq!(
        files.trace,
        [
            CaptureEpochOperatorFile::Snapshot,
            CaptureEpochOperatorFile::Witness,
            CaptureEpochOperatorFile::Snapshot
        ]
    );
}
