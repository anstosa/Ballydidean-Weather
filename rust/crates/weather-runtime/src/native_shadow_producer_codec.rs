//! Native producer admission joined to the concrete retained shadow wire codec.

use weather_contracts::{
    AdjustmentMaintenancePublishReceipt, AdjustmentMaintenanceStageInput,
    AdjustmentRevisionCommitReceipt, FieldPresence, JsonValue,
};
use weather_database::Sha256Digest;
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, PrivateShadowArchiveCodec,
    PrivateShadowCapsuleDocument, PrivateShadowPublishReceiptIdentity, PrivateShadowReceiptCodec,
    PrivateShadowStageDocument, PrivateShadowStageReceiptIdentity, RevisionArchiveCanonicalCodec,
};

use crate::NativeWirePrimitives;
use crate::api_maintenance_admission::ApiMaintenanceShadowEvidenceValues;
use crate::api_maintenance_receipts::{
    ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar,
};
use crate::api_shadow_comparator_authority::admit_shadow_comparator_authority;
use crate::api_shadow_comparator_validation::validate_shadow_comparator_document;
use crate::api_shadow_metadata_projection::shadow_prediction_metadata;
use crate::api_shadow_source_validation::validate_shadow_source;
use crate::api_shadow_wire_validation::{
    SHADOW_BODY_MAX_BYTES, SHADOW_SOURCE_MAX_BYTES, validate_shadow_body,
};
use crate::maintenance_canonical_json::field;

const SHADOW_COMPARATOR_MAX_BYTES: usize = 512 * 1_024;
const JAVASCRIPT_SAFE_UNSIGNED_MAXIMUM: u64 = 9_007_199_254_740_991;

/// Preserve the remaining producer-specific primitive beyond the shared archive effects.
pub trait NativeShadowProducerPrimitives: ArchiveWirePrimitives {
    /// Apply the original runtime bundle's bounded source-string filter.
    fn check_legacy_wind_runtime_value(&self, code_units: &[u16]) -> Option<()>;
}

/// Bind one primitive owner to the exact Node producer grammar and retained archive wire.
pub struct NativeShadowProducerCodec<P> {
    wire: ArchiveWireCodec<P>,
}

impl<P> NativeShadowProducerCodec<P>
where
    P: NativeShadowProducerPrimitives,
{
    /// Construct the producer and receipt codec without duplicating stateful primitives.
    pub const fn new(primitives: P) -> Self {
        Self {
            wire: ArchiveWireCodec::new(primitives),
        }
    }
}

impl<P> RevisionArchiveCanonicalCodec for NativeShadowProducerCodec<P>
where
    P: NativeShadowProducerPrimitives,
{
    /// Forward UTC admission to the concrete retained wire codec.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        self.wire.canonical_utc_milliseconds(text)
    }

    /// Forward exact-byte hashing to the concrete retained wire codec.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        self.wire.sha256(bytes)
    }
}

impl<P> PrivateShadowReceiptCodec for NativeShadowProducerCodec<P>
where
    P: NativeShadowProducerPrimitives,
{
    /// Delegate database receipt admission to the reviewed wire codec.
    fn admit_revision_receipt(&self, receipt: &AdjustmentRevisionCommitReceipt) -> bool {
        self.wire.admit_revision_receipt(receipt)
    }

    /// Delegate the inclusive producer commit-clock check.
    fn admit_prediction_commit_time(
        &self,
        committed_at: &str,
        issued_at: &str,
        archive_committed_at: &str,
    ) -> bool {
        self.wire
            .admit_prediction_commit_time(committed_at, issued_at, archive_committed_at)
    }

    /// Delegate exact stage receipt identity encoding.
    fn stage_receipt_identity_bytes(
        &self,
        identity: &PrivateShadowStageReceiptIdentity,
    ) -> Option<Vec<u8>> {
        self.wire.stage_receipt_identity_bytes(identity)
    }

    /// Delegate exact retained stage encoding.
    fn encode_stage_document(&self, document: &PrivateShadowStageDocument) -> Option<Vec<u8>> {
        self.wire.encode_stage_document(document)
    }

    /// Delegate exact retained stage decoding.
    fn decode_stage_document(&self, bytes: &[u8]) -> Option<PrivateShadowStageDocument> {
        self.wire.decode_stage_document(bytes)
    }

    /// Delegate exact retained capsule encoding.
    fn encode_capsule_document(&self, document: &PrivateShadowCapsuleDocument) -> Option<Vec<u8>> {
        self.wire.encode_capsule_document(document)
    }

    /// Delegate exact retained capsule decoding.
    fn decode_capsule_document(&self, bytes: &[u8]) -> Option<PrivateShadowCapsuleDocument> {
        self.wire.decode_capsule_document(bytes)
    }

    /// Delegate exact publication receipt identity encoding.
    fn publish_receipt_identity_bytes(
        &self,
        identity: &PrivateShadowPublishReceiptIdentity,
    ) -> Option<Vec<u8>> {
        self.wire.publish_receipt_identity_bytes(identity)
    }

    /// Delegate exact publication receipt encoding.
    fn encode_publish_receipt(
        &self,
        receipt: &AdjustmentMaintenancePublishReceipt,
    ) -> Option<Vec<u8>> {
        self.wire.encode_publish_receipt(receipt)
    }

    /// Delegate exact publication receipt decoding.
    fn decode_publish_receipt(&self, bytes: &[u8]) -> Option<AdjustmentMaintenancePublishReceipt> {
        self.wire.decode_publish_receipt(bytes)
    }

    /// Delegate admitted database receipt encoding.
    fn encode_revision_receipt(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>> {
        self.wire.encode_revision_receipt(receipt)
    }

    /// Delegate frontier-successor derivation and encoding.
    fn encode_revision_successor(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>> {
        self.wire.encode_revision_successor(receipt)
    }
}

impl<P> PrivateShadowArchiveCodec for NativeShadowProducerCodec<P>
where
    P: NativeShadowProducerPrimitives,
{
    /// Reproduce the complete web producer's source, body, comparator, and metadata admission.
    fn admit_stage_input(&self, input: &AdjustmentMaintenanceStageInput) -> bool {
        admit_stage_input(&ProducerEvidence(self.wire.primitives()), input).is_some()
    }
}

/// Adapt shared archive effects to the API validators without duplicating their owner.
struct ProducerEvidence<'a, P>(&'a P);

impl<P: NativeShadowProducerPrimitives> ApiMaintenanceReceiptCanonicalValues
    for ProducerEvidence<'_, P>
{
    /// Reuse the archive runtime's canonical UTC parser.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        self.0.canonical_utc_milliseconds(value)
    }

    /// Reuse the archive runtime's SHA-256 primitive and admit its lowercase wire spelling.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        Some(Sha256Digest::admitted(lower_hex(self.0.sha256(bytes))))
    }

    /// Encode the original ordered receipt object and hash its final newline.
    fn unsigned_receipt_sha256(
        &self,
        fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        let mut members = Vec::with_capacity(fields.len());
        // retain the caller-supplied receipt field order without a second JSON implementation
        for (name, value) in fields {
            members.push((name.encode_utf16().collect(), scalar_json(*value)?));
        }
        let bytes = self.0.encode_ordered_json(&JsonValue::Object(members))?;
        Some(Sha256Digest::admitted(lower_hex(self.0.sha256(&bytes))))
    }
}

impl ApiMaintenanceReceiptCanonicalValues for NativeWirePrimitives {
    /// Admit the exact canonical native UTC receipt witness.
    fn canonical_utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        self.canonical_utc_milliseconds(value)
    }

    /// Hash the observed raw bytes without interpreting their content.
    fn sha256(&self, bytes: &[u8]) -> Option<Sha256Digest> {
        Some(Sha256Digest::admitted(lower_hex(
            RevisionArchiveCanonicalCodec::sha256(self, bytes),
        )))
    }

    /// Hash the source-ordered scalar receipt using the shared JSON owner and final LF.
    fn unsigned_receipt_sha256(
        &self,
        fields: &[(&str, MaintenanceReceiptScalar<'_>)],
    ) -> Option<Sha256Digest> {
        let members = fields
            .iter()
            .map(|(name, value)| Some((name.encode_utf16().collect(), scalar_json(*value)?)))
            .collect::<Option<Vec<_>>>()?;
        let bytes = self.encode_ordered_json(&JsonValue::Object(members))?;
        ApiMaintenanceReceiptCanonicalValues::sha256(self, &bytes)
    }
}

impl<P: NativeShadowProducerPrimitives> ApiMaintenanceShadowEvidenceValues
    for ProducerEvidence<'_, P>
{
    /// Reuse the shared archive JSON decoder over exact input bytes.
    fn decode_shadow_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.0.decode_json(bytes)
    }

    /// Remove only the archive serializer's required final newline for nested preimages.
    fn encode_shadow_ordered_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        let mut encoded = self.0.encode_ordered_json(document)?;
        // reuse the allocated buffer instead of copying every ordered document
        (encoded.pop() == Some(b'\n')).then_some(encoded)
    }

    /// Reuse the shared canonical padded-base64 decoder.
    fn decode_shadow_comparator_base64(&self, encoded: &[u16]) -> Option<Vec<u8>> {
        let encoded = String::from_utf16(encoded).ok()?;
        let decoded = self.0.decode_standard_base64(&encoded)?;
        // prove canonical padding and alphabet through the same primitive owner
        (self.0.encode_standard_base64(&decoded)?.as_str() == encoded.as_str()).then_some(decoded)
    }

    /// Reuse the archive serializer's recursively sorted newline-framed JSON.
    fn encode_shadow_sorted_json(&self, document: &JsonValue) -> Option<Vec<u8>> {
        self.0.encode_sorted_json(document)
    }

    /// Forward the legacy runtime source filter.
    fn check_legacy_wind_runtime_value(&self, code_units: &[u16]) -> Option<()> {
        self.0.check_legacy_wind_runtime_value(code_units)
    }
}

/// Validate one exact producer stage without applying the API-only capture epoch gate.
fn admit_stage_input(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    input: &AdjustmentMaintenanceStageInput,
) -> Option<()> {
    let source = decode_canonical_shadow(values, &input.sourceProjection, SHADOW_SOURCE_MAX_BYTES)?;
    validate_shadow_source(values, &source, input.sourceProjection.len())?;
    let body = decode_canonical_shadow(values, &input.body, SHADOW_BODY_MAX_BYTES)?;
    validate_shadow_body(values, &body, input.body.len())?;
    let metadata =
        shadow_prediction_metadata(values, &source, &body, &input.sourceProjection, &input.body)?;
    // require every compact metadata field to be reproduced from the exact source and body bytes
    if metadata != input.metadata {
        return None;
    }
    // bind the additive incumbent member through the same complete comparator parser
    if let FieldPresence::Value(comparator) = &input.comparator {
        let document = decode_canonical_shadow(values, comparator, SHADOW_COMPARATOR_MAX_BYTES)?;
        let family = validate_shadow_comparator_document(&document, &body, &metadata)?;
        let authority = field(&document, "servingAuthority")?;
        admit_shadow_comparator_authority(values, authority, family)?;
    }
    Some(())
}

/// Decode one newline-framed JSON document and prove its producer field order byte for byte.
fn decode_canonical_shadow(
    values: &impl ApiMaintenanceShadowEvidenceValues,
    bytes: &[u8],
    maximum_bytes: usize,
) -> Option<weather_contracts::JsonValue> {
    // bound decoding before allocation and retain the producer's required final newline
    if bytes.len() > maximum_bytes || !bytes.ends_with(b"\n") {
        return None;
    }
    let document = values.decode_shadow_json(bytes)?;
    let encoded = values.encode_shadow_ordered_json(&document)?;
    (bytes.strip_suffix(b"\n") == Some(encoded.as_slice())).then_some(document)
}

/// Map one API receipt scalar through the generated JSON value contract.
fn scalar_json(value: MaintenanceReceiptScalar<'_>) -> Option<JsonValue> {
    match value {
        MaintenanceReceiptScalar::Text(value) => {
            Some(JsonValue::String(value.encode_utf16().collect()))
        }
        MaintenanceReceiptScalar::Boolean(value) => Some(JsonValue::Boolean(value)),
        MaintenanceReceiptScalar::TextArray(values) => Some(JsonValue::Array(
            values
                .iter()
                .map(|value| JsonValue::String(value.encode_utf16().collect()))
                .collect(),
        )),
        MaintenanceReceiptScalar::Unsigned(value) => {
            (value <= JAVASCRIPT_SAFE_UNSIGNED_MAXIMUM).then_some(JsonValue::Number(value as f64))
        }
        MaintenanceReceiptScalar::Null => Some(JsonValue::Null),
    }
}

/// Render one primitive-owned SHA-256 digest in the contract's lowercase wire alphabet.
fn lower_hex(bytes: [u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    // emit exactly two lowercase code points per digest byte
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}
