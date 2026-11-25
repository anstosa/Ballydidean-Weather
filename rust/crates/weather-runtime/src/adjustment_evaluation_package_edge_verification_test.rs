use std::collections::BTreeMap;
use std::fmt::{self, Display};

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use super::*;

const OBJECT_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OBJECT_MEMBER_SHA256: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const RECEIPT_MEMBER_SHA256: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const RECEIPT_IDENTITY_SHA256: &str =
    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const WATERMARK_SHA256: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const EMPTY_WATERMARK_SHA256: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SETTINGS_SHA256: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const COMPRESSED_OBJECT: &[u8] = b"deterministic-gzip";
const OBJECT_BYTES: &[u8] = b"canonical-edge-object";
const RECEIPT_BYTES: &[u8] = b"canonical-edge-receipt\n";
const STABLE_BYTES: &[u8] = b"canonical-stable-identity";
const WATERMARK_BYTES: &[u8] = b"canonical-watermark";
const EMPTY_WATERMARK_BYTES: &[u8] = b"canonical-empty-watermark";

#[derive(Debug, Clone, Copy)]
struct FixtureError;

impl Display for FixtureError {
    /// render one unreachable fixture effect failure
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fixture member is unavailable")
    }
}

struct Values {
    canonical: Vec<(JsonValue, Vec<u8>)>,
    decoded: BTreeMap<Vec<u8>, JsonValue>,
    hashes: BTreeMap<Vec<u8>, Sha256Digest>,
    members: BTreeMap<String, Vec<u8>>,
}

impl Values {
    /// build the exact edge pair codec fixture
    fn matching() -> Self {
        let object = object();
        let receipt = receipt();
        let stable = stable_identity();
        let watermark = watermark();
        let canonical = vec![
            (object.clone(), framed(OBJECT_BYTES)),
            (receipt.clone(), RECEIPT_BYTES.to_vec()),
            (stable, framed(STABLE_BYTES)),
            (watermark, framed(WATERMARK_BYTES)),
            (JsonValue::Array(Vec::new()), framed(EMPTY_WATERMARK_BYTES)),
        ];
        let decoded = BTreeMap::from([
            (OBJECT_BYTES.to_vec(), object),
            (RECEIPT_BYTES.to_vec(), receipt),
        ]);
        let hashes = BTreeMap::from([
            (
                COMPRESSED_OBJECT.to_vec(),
                Sha256Digest::admitted(OBJECT_MEMBER_SHA256),
            ),
            (OBJECT_BYTES.to_vec(), Sha256Digest::admitted(OBJECT_SHA256)),
            (
                RECEIPT_BYTES.to_vec(),
                Sha256Digest::admitted(RECEIPT_MEMBER_SHA256),
            ),
            (
                STABLE_BYTES.to_vec(),
                Sha256Digest::admitted(RECEIPT_IDENTITY_SHA256),
            ),
            (
                WATERMARK_BYTES.to_vec(),
                Sha256Digest::admitted(WATERMARK_SHA256),
            ),
            (
                EMPTY_WATERMARK_BYTES.to_vec(),
                Sha256Digest::admitted(EMPTY_WATERMARK_SHA256),
            ),
        ]);
        let members = BTreeMap::from([
            (object_path(), COMPRESSED_OBJECT.to_vec()),
            (receipt_path(), RECEIPT_BYTES.to_vec()),
        ]);
        Self {
            canonical,
            decoded,
            hashes,
            members,
        }
    }
}

impl MaintenanceCanonicalJson for Values {
    /// parse only the canonical fixture documents
    fn parse_canonical_object(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.decoded.get(bytes).cloned()
    }

    /// encode only the fixture's exact semantic values
    fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
        self.canonical
            .iter()
            .find_map(|(candidate, bytes)| (candidate == value).then(|| bytes.clone()))
    }

    /// hash only byte populations with fixture identities
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
        self.hashes.get(bytes).cloned()
    }
}

impl AdjustmentEvaluationPackageRowValues for Values {
    /// decode only the fixture object or receipt
    fn decode_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.decoded.get(bytes).cloned()
    }

    /// parse only the fixture receipt instant
    fn js_timestamp_milliseconds(&mut self, value: &JsonValue) -> Option<i64> {
        (value == &json_string("2026-10-01T00:00:00.000Z")).then_some(0)
    }

    /// leave row-local dates outside this edge fixture
    fn matches_canonical_local_date(&mut self, _: &JsonValue) -> bool {
        false
    }

    /// leave row payload inspection outside this edge fixture
    fn payload_contains_forbidden_private_material(&mut self, _: &JsonValue) -> bool {
        false
    }
}

impl AdjustmentEvaluationEdgeVerificationValues for Values {
    type Error = FixtureError;

    /// read only one declared fixture member under its ceiling
    fn read_bound_edge_member(
        &mut self,
        root: &str,
        member: &AdjustmentEvaluationEdgeMember,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, Self::Error> {
        // bind every read to the explicit package root
        if root != "/fixture/adjustment" {
            return Err(FixtureError);
        }
        let bytes = self.members.get(&member.path).ok_or(FixtureError)?;
        // preserve the requested fixed allocation ceiling
        if bytes.len() > maximum_bytes {
            return Err(FixtureError);
        }
        Ok(bytes.clone())
    }

    /// inflate only the declared deterministic object bytes
    fn gunzip_bounded(
        &mut self,
        compressed: &[u8],
        maximum_output_bytes: usize,
    ) -> Option<Vec<u8>> {
        // preserve the caller's required output ceiling
        if compressed != COMPRESSED_OBJECT || OBJECT_BYTES.len() > maximum_output_bytes {
            return None;
        }
        Some(OBJECT_BYTES.to_vec())
    }

    /// reproduce only the exact deterministic compressed fixture
    fn gzip_deterministic(&mut self, plaintext: &[u8]) -> Option<Vec<u8>> {
        (plaintext == OBJECT_BYTES).then(|| COMPRESSED_OBJECT.to_vec())
    }
}

/// verify one complete receipt/object pair and its ordered watermark
#[test]
fn verifies_one_frozen_edge_pair_and_returns_both_paths() {
    let mut values = Values::matching();
    let object = object_member();
    let receipt = receipt_member();

    let verified = verify_adjustment_evaluation_edge_bundle(
        "/fixture/adjustment",
        &summary(),
        std::slice::from_ref(&object),
        std::slice::from_ref(&receipt),
        &mut values,
    )
    .expect("complete edge pair should verify");

    assert_eq!(verified, vec![object.path, receipt.path]);
}

/// reject one receipt whose declared object is absent
#[test]
fn rejects_receipt_when_its_declared_object_is_missing() {
    let mut values = Values::matching();
    let receipt = receipt_member();
    let summary = AdjustmentEvaluationEdgeSummary {
        object_count: 0,
        receipt_count: 1,
        watermark_sha256: Sha256Digest::admitted(WATERMARK_SHA256),
    };

    assert_eq!(
        verify_adjustment_evaluation_edge_bundle(
            "/fixture/adjustment",
            &summary,
            &[],
            &[receipt],
            &mut values
        ),
        Err(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)
    );
}

/// verify a declared object that no receipt references
#[test]
fn verifies_unreferenced_declared_object_after_empty_watermark() {
    let mut values = Values::matching();
    let object = object_member();
    let summary = AdjustmentEvaluationEdgeSummary {
        object_count: 1,
        receipt_count: 0,
        watermark_sha256: Sha256Digest::admitted(EMPTY_WATERMARK_SHA256),
    };

    assert_eq!(
        verify_adjustment_evaluation_edge_bundle(
            "/fixture/adjustment",
            &summary,
            std::slice::from_ref(&object),
            &[],
            &mut values
        ),
        Ok(vec![object.path])
    );
}

/// build the manifest edge summary
fn summary() -> AdjustmentEvaluationEdgeSummary {
    AdjustmentEvaluationEdgeSummary {
        object_count: 1,
        receipt_count: 1,
        watermark_sha256: Sha256Digest::admitted(WATERMARK_SHA256),
    }
}

/// build the declared edge object member
fn object_member() -> AdjustmentEvaluationEdgeMember {
    AdjustmentEvaluationEdgeMember {
        path: object_path(),
        sha256: Sha256Digest::admitted(OBJECT_MEMBER_SHA256),
        size_bytes: COMPRESSED_OBJECT.len() as u64,
    }
}

/// build the declared edge receipt member
fn receipt_member() -> AdjustmentEvaluationEdgeMember {
    AdjustmentEvaluationEdgeMember {
        path: receipt_path(),
        sha256: Sha256Digest::admitted(RECEIPT_MEMBER_SHA256),
        size_bytes: RECEIPT_BYTES.len() as u64,
    }
}

/// return the declared object path
fn object_path() -> String {
    format!("members/edge/objects/sha256-{OBJECT_SHA256}.json.gz")
}

/// return the declared receipt path
fn receipt_path() -> String {
    format!("members/edge/receipts/sha256-{RECEIPT_IDENTITY_SHA256}.json")
}

/// build the canonical edge object value
fn object() -> JsonValue {
    json_object(vec![
        ("bundleIdentities", json_object(Vec::new())),
        ("contractVersion", json_string(EDGE_OBJECT_VERSION)),
        (
            "rows",
            JsonValue::Array(vec![json_object(vec![
                (
                    "record",
                    json_object(vec![("validAt", json_string("fixture"))]),
                ),
                (
                    "source",
                    json_object(vec![("kind", json_string("fixture"))]),
                ),
            ])]),
        ),
        ("settingsSha256", json_string(SETTINGS_SHA256)),
        ("siteKey", json_string("ballydidean")),
        ("window", json_string("days=1")),
    ])
}

/// build the canonical edge receipt value
fn receipt() -> JsonValue {
    json_object(vec![
        (
            "availability",
            json_object(vec![
                (
                    "rowTimestampIndexes",
                    JsonValue::Array(vec![JsonValue::Number(0.0)]),
                ),
                (
                    "timestamps",
                    JsonValue::Array(vec![json_string("2026-10-01T00:00:00.000Z")]),
                ),
            ]),
        ),
        ("contractVersion", json_string(EDGE_RECEIPT_VERSION)),
        (
            "edgeReceiptIdentitySha256",
            json_string(RECEIPT_IDENTITY_SHA256),
        ),
        (
            "firstEdgeCommittedAt",
            json_string("2026-10-01T00:00:00.000Z"),
        ),
        ("objectSha256", json_string(OBJECT_SHA256)),
        ("siteKey", json_string("ballydidean")),
        ("window", json_string("days=1")),
    ])
}

/// build the stable identity preimage value
fn stable_identity() -> JsonValue {
    let object = object();
    let rows = json_array(&object, "rows").expect("fixture rows");
    let record = field(&rows[0], "record").expect("fixture record");
    let source = field(&rows[0], "source").expect("fixture source");
    json_object(vec![
        ("bundleIdentities", json_object(Vec::new())),
        (
            "rows",
            JsonValue::Array(vec![json_object(vec![
                ("record", record.clone()),
                ("source", source.clone()),
            ])]),
        ),
        ("settingsSha256", json_string(SETTINGS_SHA256)),
        ("siteKey", json_string("ballydidean")),
        ("window", json_string("days=1")),
    ])
}

/// build the ordered watermark preimage value
fn watermark() -> JsonValue {
    JsonValue::Array(vec![json_object(vec![
        (
            "edgeReceiptIdentitySha256",
            json_string(RECEIPT_IDENTITY_SHA256),
        ),
        ("objectSha256", json_string(OBJECT_SHA256)),
    ])])
}

/// append the canonical record delimiter
fn framed(value: &[u8]) -> Vec<u8> {
    let mut output = value.to_vec();
    output.push(b'\n');
    output
}
