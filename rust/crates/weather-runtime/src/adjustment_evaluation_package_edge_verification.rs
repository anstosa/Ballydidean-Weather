//! strict verification for frozen adjustment-evaluation edge members

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display};

use weather_contracts::JsonValue;
use weather_database::Sha256Digest;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{AdjustmentEvaluationPackageRowValues, MaintenanceCanonicalJson};

const EDGE_OBJECT_VERSION: &str = "forecast-adjustment-evidence-object/v1";
const EDGE_RECEIPT_VERSION: &str = "forecast-adjustment-edge-receipt/v1";
const MAXIMUM_EDGE_MEMBER_BYTES: u64 = 16 * 1024;
const MAXIMUM_EDGE_OBJECT_PLAINTEXT_BYTES: usize = 512 * 1024 + 1;
const MAXIMUM_DECOMPRESSION_RATIO: usize = 64;
const MAXIMUM_EDGE_OBJECT_ROWS: usize = 240;
const MAXIMUM_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// one manifest-bound frozen edge member descriptor
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustmentEvaluationEdgeMember {
    pub path: String,
    pub sha256: Sha256Digest,
    pub size_bytes: u64,
}

/// the edge identity declared by the admitted package manifest
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustmentEvaluationEdgeSummary {
    pub object_count: u64,
    pub receipt_count: u64,
    pub watermark_sha256: Sha256Digest,
}

/// approved member, JSON, gzip, digest, and timestamp owners
pub trait AdjustmentEvaluationEdgeVerificationValues: AdjustmentEvaluationPackageRowValues {
    type Error: Display;

    /// read and rebind one member under the caller's fixed ceiling
    fn read_bound_edge_member(
        &mut self,
        root: &str,
        member: &AdjustmentEvaluationEdgeMember,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, Self::Error>;

    /// inflate one object under the predecessor output ceiling
    fn gunzip_bounded(&mut self, compressed: &[u8], maximum_output_bytes: usize)
    -> Option<Vec<u8>>;

    /// reproduce the predecessor's deterministic level-nine gzip bytes
    fn gzip_deterministic(&mut self, plaintext: &[u8]) -> Option<Vec<u8>>;
}

/// closed failures from the frozen edge verification phase
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdjustmentEvaluationEdgeVerificationError {
    EdgeCounts,
    Effect(String),
    MemberBytes,
    MemberSizeLimit,
    ObjectShape(&'static str),
    UnexpectedKeys(&'static str),
    ObjectBytes,
    ReceiptAvailability,
    ReceiptIdentity,
    Watermark,
}

impl Display for AdjustmentEvaluationEdgeVerificationError {
    /// render one predecessor-compatible ordinary diagnostic
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // preserve the original phase diagnostics
        match self {
            Self::EdgeCounts => formatter.write_str("edge evidence member counts differ"),
            Self::Effect(message) => formatter.write_str(message),
            Self::MemberBytes => formatter.write_str("package member bytes differ"),
            Self::MemberSizeLimit => {
                formatter.write_str("package member exceeds its fixed size limit")
            }
            Self::ObjectShape(label) => write!(formatter, "{label} must be an object"),
            Self::UnexpectedKeys(label) => write!(formatter, "{label} has unexpected keys"),
            Self::ObjectBytes => formatter.write_str("edge object bytes differ"),
            Self::ReceiptAvailability => {
                formatter.write_str("edge receipt availability is invalid")
            }
            Self::ReceiptIdentity => formatter.write_str("edge receipt identity is invalid"),
            Self::Watermark => formatter.write_str("edge evidence watermark differs"),
        }
    }
}

impl std::error::Error for AdjustmentEvaluationEdgeVerificationError {}

/// verify every receipt/object pair and return every verified member path
pub fn verify_adjustment_evaluation_edge_bundle(
    root: &str,
    summary: &AdjustmentEvaluationEdgeSummary,
    object_members: &[AdjustmentEvaluationEdgeMember],
    receipt_members: &[AdjustmentEvaluationEdgeMember],
    values: &mut impl AdjustmentEvaluationEdgeVerificationValues,
) -> Result<Vec<String>, AdjustmentEvaluationEdgeVerificationError> {
    // bind manifest counts before opening any pair
    if usize::try_from(summary.object_count).ok() != Some(object_members.len())
        || usize::try_from(summary.receipt_count).ok() != Some(receipt_members.len())
    {
        return Err(AdjustmentEvaluationEdgeVerificationError::EdgeCounts);
    }

    let object_members_by_path = object_members
        .iter()
        .map(|member| (member.path.as_str(), member))
        .collect::<BTreeMap<_, _>>();
    // reject duplicate declarations hidden by lookup replacement
    if object_members_by_path.len() != object_members.len() {
        return Err(AdjustmentEvaluationEdgeVerificationError::EdgeCounts);
    }

    let mut validated_objects = BTreeMap::<String, VerifiedEdgeObject>::new();
    let mut verified_paths = BTreeSet::<String>::new();
    let mut watermark_pairs = Vec::with_capacity(receipt_members.len());

    // preserve manifest receipt order for the portable watermark
    for receipt_member in receipt_members {
        let receipt = verify_receipt_member(root, receipt_member, values)?;
        let object_path = format!(
            "members/edge/objects/sha256-{}.json.gz",
            receipt.object_sha256
        );
        let object_member = object_members_by_path
            .get(object_path.as_str())
            .ok_or(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)?;

        // validate each content-addressed object only once
        let object = match validated_objects.entry(object_path.clone()) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let object =
                    verify_object_member(root, object_member, &receipt.object_sha256, values)?;
                verified_paths.insert(object_path.clone());
                entry.insert(object)
            }
        };

        // bind the receipt to the exact object rows and window
        if receipt.identity_sha256 != object.stable_identity_sha256
            || receipt.window != object.window
            || receipt.row_timestamp_indexes.len() != object.row_count
            || receipt
                .row_timestamp_indexes
                .iter()
                .any(|index| !valid_index(*index, receipt.timestamp_count))
        {
            return Err(AdjustmentEvaluationEdgeVerificationError::ReceiptAvailability);
        }

        watermark_pairs.push(json_object(vec![
            (
                "edgeReceiptIdentitySha256",
                json_string(&receipt.identity_sha256),
            ),
            ("objectSha256", json_string(&receipt.object_sha256)),
        ]));
        verified_paths.insert(receipt_member.path.clone());
    }

    let watermark_value = JsonValue::Array(watermark_pairs);
    let watermark_bytes = canonical_value_bytes(values, &watermark_value)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::Watermark)?;
    let watermark_sha256 = values
        .sha256(&watermark_bytes)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::Watermark)?;
    // bind ordered receipt/object pairs before unrelated object checks
    if watermark_sha256 != summary.watermark_sha256 {
        return Err(AdjustmentEvaluationEdgeVerificationError::Watermark);
    }

    // validate every declared object even when no receipt references it
    for object_member in object_members {
        // finish any object omitted from receipt traversal
        if !verified_paths.contains(&object_member.path) {
            let expected_sha256 = object_sha256_from_path(&object_member.path)
                .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?;
            verify_object_member(root, object_member, expected_sha256, values)?;
            verified_paths.insert(object_member.path.clone());
        }
    }

    Ok(verified_paths.into_iter().collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifiedEdgeObject {
    row_count: usize,
    stable_identity_sha256: String,
    window: String,
}

#[derive(Debug, Clone, PartialEq)]
struct VerifiedEdgeReceipt {
    identity_sha256: String,
    object_sha256: String,
    row_timestamp_indexes: Vec<f64>,
    timestamp_count: usize,
    window: String,
}

/// verify one receipt and retain only its object binding
fn verify_receipt_member(
    root: &str,
    member: &AdjustmentEvaluationEdgeMember,
    values: &mut impl AdjustmentEvaluationEdgeVerificationValues,
) -> Result<VerifiedEdgeReceipt, AdjustmentEvaluationEdgeVerificationError> {
    let bytes = read_member_bytes(root, member, values)?;
    let receipt = values
        .decode_json(&bytes)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)?;
    require_edge_fields(&receipt, RECEIPT_KEYS, "edge receipt")?;
    let availability =
        field(&receipt, "availability").expect("exact receipt keys include availability");
    require_edge_fields(availability, AVAILABILITY_KEYS, "edge receipt availability")?;
    let identity_sha256 = receipt_sha256_from_path(&member.path)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)?;
    let declared_identity = text(&receipt, "edgeReceiptIdentitySha256")
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)?;
    let object_sha256 = text(&receipt, "objectSha256")
        .filter(|value| valid_hash(value))
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)?;
    let window = text(&receipt, "window")
        .filter(|value| valid_window(value))
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)?;
    let first_committed_at = field(&receipt, "firstEdgeCommittedAt")
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)?;
    let timestamps = json_array(availability, "timestamps")
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)?;
    let row_timestamp_indexes = json_array(availability, "rowTimestampIndexes")
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity)?;
    // bind canonical bytes, identity, timestamps, and contract fields together
    if declared_identity != identity_sha256
        || text(&receipt, "contractVersion").as_deref() != Some(EDGE_RECEIPT_VERSION)
        || text(&receipt, "siteKey").as_deref() != Some("ballydidean")
        || values
            .js_timestamp_milliseconds(first_committed_at)
            .is_none()
        || timestamps
            .iter()
            .any(|value| values.js_timestamp_milliseconds(value).is_none())
        || values.encode_canonical_json(&receipt).as_deref() != Some(bytes.as_slice())
    {
        return Err(AdjustmentEvaluationEdgeVerificationError::ReceiptIdentity);
    }

    Ok(VerifiedEdgeReceipt {
        identity_sha256,
        object_sha256,
        row_timestamp_indexes: row_timestamp_indexes
            .iter()
            .map(|value| match value {
                JsonValue::Number(index) => *index,
                _ => f64::NAN,
            })
            .collect(),
        timestamp_count: timestamps.len(),
        window,
    })
}

/// verify one deterministic object and derive its stable receipt identity
fn verify_object_member(
    root: &str,
    member: &AdjustmentEvaluationEdgeMember,
    expected_sha256: &str,
    values: &mut impl AdjustmentEvaluationEdgeVerificationValues,
) -> Result<VerifiedEdgeObject, AdjustmentEvaluationEdgeVerificationError> {
    let compressed = read_member_bytes(root, member, values)?;
    let plaintext = values
        .gunzip_bounded(&compressed, MAXIMUM_EDGE_OBJECT_PLAINTEXT_BYTES)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?;
    let object = values
        .decode_json(&plaintext)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?;
    require_edge_fields(&object, OBJECT_KEYS, "edge object")?;
    let rows = json_array(&object, "rows")
        .filter(|rows| !rows.is_empty() && rows.len() <= MAXIMUM_EDGE_OBJECT_ROWS)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?;
    let settings_sha256 = text(&object, "settingsSha256")
        .filter(|value| valid_hash(value))
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?;
    let window = text(&object, "window")
        .filter(|value| valid_window(value))
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?;
    let ratio_limit = compressed
        .len()
        .checked_mul(MAXIMUM_DECOMPRESSION_RATIO)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?;

    // bind object contract, bytes, deterministic compression, and address
    if text(&object, "contractVersion").as_deref() != Some(EDGE_OBJECT_VERSION)
        || text(&object, "siteKey").as_deref() != Some("ballydidean")
        || plaintext.len() > ratio_limit
        || canonical_value_bytes(values, &object).as_deref() != Some(plaintext.as_slice())
        || values.gzip_deterministic(&plaintext).as_deref() != Some(compressed.as_slice())
        || !values
            .sha256(&plaintext)
            .is_some_and(|digest| digest.as_str() == expected_sha256)
    {
        return Err(AdjustmentEvaluationEdgeVerificationError::ObjectBytes);
    }

    let mut stable_rows = Vec::with_capacity(rows.len());
    // retain only the stable record/source projection from each row
    for row in rows {
        require_edge_object(row, "edge object row")?;
        let record = field(row, "record")
            .filter(|value| is_object(value))
            .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectShape(
                "edge object row record",
            ))?;
        let source = field(row, "source")
            .filter(|value| is_object(value))
            .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectShape(
                "edge object row source",
            ))?;
        stable_rows.push(json_object(vec![
            ("record", record.clone()),
            ("source", source.clone()),
        ]));
    }
    let stable_value = json_object(vec![
        (
            "bundleIdentities",
            field(&object, "bundleIdentities")
                .cloned()
                .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?,
        ),
        ("rows", JsonValue::Array(stable_rows)),
        ("settingsSha256", json_string(&settings_sha256)),
        ("siteKey", json_string("ballydidean")),
        ("window", json_string(&window)),
    ]);
    let stable_bytes = canonical_value_bytes(values, &stable_value)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?;
    let stable_identity_sha256 = values
        .sha256(&stable_bytes)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::ObjectBytes)?;

    Ok(VerifiedEdgeObject {
        row_count: rows.len(),
        stable_identity_sha256: stable_identity_sha256.as_str().to_owned(),
        window,
    })
}

/// preserve the predecessor's ordinary object type diagnostic
fn require_edge_object(
    value: &JsonValue,
    label: &'static str,
) -> Result<(), AdjustmentEvaluationEdgeVerificationError> {
    // reject non-object input before considering its keys
    if !is_object(value) {
        return Err(AdjustmentEvaluationEdgeVerificationError::ObjectShape(
            label,
        ));
    }
    Ok(())
}

/// require the original exact object key set
fn require_edge_fields(
    value: &JsonValue,
    keys: &[&str],
    label: &'static str,
) -> Result<(), AdjustmentEvaluationEdgeVerificationError> {
    require_edge_object(value, label)?;
    // keep the original key error distinct from its object type error
    if !exact_keys(value, keys) {
        return Err(AdjustmentEvaluationEdgeVerificationError::UnexpectedKeys(
            label,
        ));
    }
    Ok(())
}

/// read and rebind one declared member under the fixed ceiling
fn read_member_bytes(
    root: &str,
    member: &AdjustmentEvaluationEdgeMember,
    values: &mut impl AdjustmentEvaluationEdgeVerificationValues,
) -> Result<Vec<u8>, AdjustmentEvaluationEdgeVerificationError> {
    // enforce the fixed read ceiling before opening content
    if member.size_bytes > MAXIMUM_EDGE_MEMBER_BYTES {
        return Err(AdjustmentEvaluationEdgeVerificationError::MemberSizeLimit);
    }
    let bytes = values
        .read_bound_edge_member(root, member, MAXIMUM_EDGE_MEMBER_BYTES as usize)
        .map_err(|error| AdjustmentEvaluationEdgeVerificationError::Effect(error.to_string()))?;
    let observed_sha256 = values
        .sha256(&bytes)
        .ok_or(AdjustmentEvaluationEdgeVerificationError::MemberBytes)?;
    // require both exact allocation and exact bytes
    if usize::try_from(member.size_bytes).ok() != Some(bytes.len())
        || observed_sha256 != member.sha256
    {
        return Err(AdjustmentEvaluationEdgeVerificationError::MemberBytes);
    }
    Ok(bytes)
}

/// encode one canonical value without the record newline
fn canonical_value_bytes(
    values: &mut impl MaintenanceCanonicalJson,
    value: &JsonValue,
) -> Option<Vec<u8>> {
    let mut bytes = values.encode_canonical_json(value)?;
    // remove only the canonical record delimiter
    if bytes.pop() != Some(b'\n') {
        return None;
    }
    Some(bytes)
}

/// recognize one predecessor edge window
fn valid_window(value: &str) -> bool {
    matches!(value, "days=1" | "days=5" | "days=10" | "overnight")
}

/// recognize one safe availability index into the timestamp array
fn valid_index(value: f64, timestamp_count: usize) -> bool {
    value.is_finite()
        && (0.0..=MAXIMUM_SAFE_INTEGER).contains(&value)
        && value.fract() == 0.0
        && value < timestamp_count as f64
}

/// decode one exact receipt filename identity
fn receipt_sha256_from_path(path: &str) -> Option<String> {
    path.strip_prefix("members/edge/receipts/sha256-")
        .and_then(|value| value.strip_suffix(".json"))
        .filter(|value| valid_hash(value))
        .map(ToOwned::to_owned)
}

/// borrow one exact object filename identity
fn object_sha256_from_path(path: &str) -> Option<&str> {
    path.strip_prefix("members/edge/objects/sha256-")
        .and_then(|value| value.strip_suffix(".json.gz"))
        .filter(|value| valid_hash(value))
}

/// borrow one required array field
fn json_array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    Some(values)
}

/// recognize one decoded object
fn is_object(value: &JsonValue) -> bool {
    matches!(value, JsonValue::Object(_))
}

/// build one verifier-owned JSON string
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// build one verifier-owned JSON object
fn json_object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}

const OBJECT_KEYS: &[&str] = &[
    "bundleIdentities",
    "contractVersion",
    "rows",
    "settingsSha256",
    "siteKey",
    "window",
];
const RECEIPT_KEYS: &[&str] = &[
    "availability",
    "contractVersion",
    "edgeReceiptIdentitySha256",
    "firstEdgeCommittedAt",
    "objectSha256",
    "siteKey",
    "window",
];
const AVAILABILITY_KEYS: &[&str] = &["rowTimestampIndexes", "timestamps"];

#[cfg(test)]
#[path = "adjustment_evaluation_package_edge_verification_test.rs"]
mod tests;
