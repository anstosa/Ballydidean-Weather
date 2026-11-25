//! Bind real prepared forecast evidence to the immutable Linux object writer.

use crate::{
    ForecastEvidenceMaterial, ForecastEvidenceObjectVersion, ForecastEvidenceReceiptCodec,
    PreparedForecastEvidence,
};

use super::forecast_objects::{
    DecodedForecastObjectReceipt, EncodedForecastObjectReceipt, ForecastObjectCommitCodec,
    ForecastObjectReceiptVersion, ForecastObjectSha256,
};

impl<Codec: ForecastEvidenceReceiptCodec> ForecastObjectCommitCodec<PreparedForecastEvidence>
    for ForecastEvidenceMaterial<Codec>
{
    /// verify the exact immutable gzip bytes before giving the writer their canonical object hash.
    fn object_sha256(
        &self,
        prepared: &PreparedForecastEvidence,
        compressed: &[u8],
    ) -> Option<ForecastObjectSha256> {
        (prepared.compressed_object() == compressed)
            .then(|| ForecastObjectSha256::admitted(prepared.object_sha256()))
    }

    /// obtain the stable record/source address already bound during preparation.
    fn edge_receipt_identity_sha256(
        &self,
        prepared: &PreparedForecastEvidence,
    ) -> Option<ForecastObjectSha256> {
        Some(ForecastObjectSha256::admitted(
            prepared.edge_receipt_identity_sha256(),
        ))
    }

    /// create one freshly observed first receipt for the already admitted paired object.
    fn encode_receipt(
        &self,
        prepared: &PreparedForecastEvidence,
        first_edge_committed_at: &str,
        object_sha256: ForecastObjectSha256,
        edge_receipt_identity_sha256: ForecastObjectSha256,
    ) -> Option<EncodedForecastObjectReceipt> {
        // retain the pair binding before returning canonical material to the filesystem owner
        if object_sha256.as_bytes() != &prepared.object_sha256()
            || edge_receipt_identity_sha256.as_bytes() != &prepared.edge_receipt_identity_sha256()
        {
            return None;
        }
        Some(EncodedForecastObjectReceipt {
            bytes: ForecastEvidenceMaterial::encode_receipt(
                self,
                prepared,
                first_edge_committed_at,
            )
            .ok()?,
            edge_receipt_identity_sha256,
            object_sha256,
            version: receipt_version(prepared.version()),
        })
    }

    /// validate the actual retained receipt without replacing its first observed fields.
    fn decode_receipt(
        &self,
        expected: ForecastObjectSha256,
        bytes: &[u8],
    ) -> Option<DecodedForecastObjectReceipt> {
        let retained = self.admit_retained_receipt(*expected.as_bytes(), bytes)?;
        Some(DecodedForecastObjectReceipt {
            object_sha256: ForecastObjectSha256::admitted(retained.object_sha256),
            version: receipt_version(retained.version),
        })
    }
}

/// retain the exact matching native and producer receipt contracts.
const fn receipt_version(version: ForecastEvidenceObjectVersion) -> ForecastObjectReceiptVersion {
    match version {
        ForecastEvidenceObjectVersion::Legacy => ForecastObjectReceiptVersion::V1,
        ForecastEvidenceObjectVersion::Extended => ForecastObjectReceiptVersion::V2,
    }
}

use weather_contracts::JsonValue;

use crate::RevisionGapSha256;
use crate::revision_archive_canonical::{instant, lower_hex};

use super::forecast_channels::{
    DecodedForecastChannelAssertion, DecodedForecastChannelBinding, DecodedForecastChannelReceipt,
    EncodedForecastChannelAssertion, EncodedForecastChannelBinding, ForecastChannelCodec,
    ForecastIssuanceChannel,
};

const ASSERTION_CONTRACT: &str = "issuance-channel-assertion/v2";
const BINDING_CONTRACT: &str = "issuance-channel-binding/v2";

impl<Codec: ForecastEvidenceReceiptCodec> ForecastChannelCodec for ForecastEvidenceMaterial<Codec> {
    /// admit the existing complete receipt without changing its canonical time.
    fn decode_receipt(
        &self,
        expected: ForecastObjectSha256,
        bytes: &[u8],
    ) -> Option<DecodedForecastChannelReceipt> {
        let receipt = self.admit_retained_receipt(*expected.as_bytes(), bytes)?;
        Some(DecodedForecastChannelReceipt {
            first_edge_committed_at: receipt.first_edge_committed_at,
        })
    }

    /// bind the exact retained receipt bytes, including their original framing.
    fn receipt_sha256(&self, bytes: &[u8]) -> ForecastObjectSha256 {
        ForecastObjectSha256::admitted(self.codec().sha256(bytes))
    }

    /// serialize one assertion and hash its original canonical frame.
    fn encode_assertion(
        &self,
        channel: ForecastIssuanceChannel,
        identity: ForecastObjectSha256,
        first_edge_committed_at: &str,
        receipt_sha256: ForecastObjectSha256,
    ) -> Option<EncodedForecastChannelAssertion> {
        instant(self.codec(), first_edge_committed_at)?;
        let value = object([
            ("channel", string(channel.wire())),
            ("contractVersion", string(ASSERTION_CONTRACT)),
            ("edgeReceiptIdentitySha256", digest(identity)),
            ("firstEdgeCommittedAt", string(first_edge_committed_at)),
            ("receiptSha256", digest(receipt_sha256)),
        ]);
        let bytes = framed(self.codec(), &value)?;
        Some(EncodedForecastChannelAssertion {
            assertion_sha256: ForecastObjectSha256::admitted(self.codec().sha256(&bytes)),
            bytes,
            channel,
            edge_receipt_identity_sha256: identity,
            first_edge_committed_at: first_edge_committed_at.to_owned(),
            receipt_sha256,
        })
    }

    /// read only the exact canonical assertion at its content-addressed filename.
    fn decode_assertion(
        &self,
        expected: ForecastObjectSha256,
        bytes: &[u8],
    ) -> Option<DecodedForecastChannelAssertion> {
        let value = self.codec().decode_retained_json(bytes)?;
        let fields = object_fields(&value)?;
        // require exact canonical bytes at the expected content address
        if fields.len() != 5
            || text(fields, "contractVersion")? != ASSERTION_CONTRACT
            || framed(self.codec(), &value)? != bytes
            || &self.codec().sha256(bytes) != expected.as_bytes()
        {
            return None;
        }
        let channel = match text(fields, "channel")?.as_str() {
            "legacy_unattributed" => ForecastIssuanceChannel::LegacyUnattributed,
            "public_get" => ForecastIssuanceChannel::PublicGet,
            "scheduler_request" => ForecastIssuanceChannel::SchedulerRequest,
            _ => return None,
        };
        let first_edge_committed_at = text(fields, "firstEdgeCommittedAt")?;
        instant(self.codec(), &first_edge_committed_at)?;
        Some(DecodedForecastChannelAssertion {
            channel,
            edge_receipt_identity_sha256: parse_digest(fields, "edgeReceiptIdentitySha256")?,
            first_edge_committed_at,
            receipt_sha256: parse_digest(fields, "receiptSha256")?,
        })
    }

    /// encode one complete stable pointer to an existing content-addressed assertion.
    fn encode_binding(
        &self,
        identity: ForecastObjectSha256,
        assertion_sha256: ForecastObjectSha256,
    ) -> Option<EncodedForecastChannelBinding> {
        let value = object([
            ("assertionSha256", digest(assertion_sha256)),
            ("contractVersion", string(BINDING_CONTRACT)),
            ("edgeReceiptIdentitySha256", digest(identity)),
        ]);
        Some(EncodedForecastChannelBinding {
            bytes: framed(self.codec(), &value)?,
            assertion_sha256,
            edge_receipt_identity_sha256: identity,
        })
    }

    /// read the original stable pointer without replacing its first assertion.
    fn decode_binding(
        &self,
        expected: ForecastObjectSha256,
        bytes: &[u8],
    ) -> Option<DecodedForecastChannelBinding> {
        let value = self.codec().decode_retained_json(bytes)?;
        let fields = object_fields(&value)?;
        // bind the exact historical record to its stable receipt and canonical frame
        if fields.len() != 3
            || text(fields, "contractVersion")? != BINDING_CONTRACT
            || parse_digest(fields, "edgeReceiptIdentitySha256")? != expected
            || framed(self.codec(), &value)? != bytes
        {
            return None;
        }
        Some(DecodedForecastChannelBinding {
            assertion_sha256: parse_digest(fields, "assertionSha256")?,
        })
    }
}

type Object = [(Vec<u16>, JsonValue)];

/// read the closed object's fields without reordering its original source.
fn object_fields(value: &JsonValue) -> Option<&Object> {
    match value {
        JsonValue::Object(fields) => Some(fields),
        _ => None,
    }
}

/// recover one closed string field from an existing document.
fn text(fields: &Object, key: &str) -> Option<String> {
    let (_, JsonValue::String(value)) = fields.iter().find(
        // compare the original UTF-16 field spelling
        |(name, _)| name.iter().copied().eq(key.encode_utf16()),
    )?
    else {
        return None;
    };
    String::from_utf16(value).ok()
}

/// recover a fixed lowercase digest without hashing or changing its wire identity.
fn parse_digest(fields: &Object, key: &str) -> Option<ForecastObjectSha256> {
    let raw = RevisionGapSha256::from_lower_hex(&text(fields, key)?)?;
    Some(ForecastObjectSha256::admitted(*raw.as_bytes()))
}

/// represent one immutable digest as its original hexadecimal string.
fn digest(sha256: ForecastObjectSha256) -> JsonValue {
    string(&lower_hex(*sha256.as_bytes()))
}

/// represent one fixed string without implementing JSON serialization.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// construct one closed ordered object for the approved JSON codec.
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(
                // construct the actual source-owned public field names
                |(name, value)| (name.encode_utf16().collect(), value),
            )
            .collect(),
    )
}

/// encode the source owner's exact canonical newline-framed document.
fn framed(codec: &impl ForecastEvidenceReceiptCodec, value: &JsonValue) -> Option<Vec<u8>> {
    let mut bytes = codec.canonical_json(value)?;
    bytes.push(b'\n');
    Some(bytes)
}

use crate::{RetainedForecastEvidenceObject, RetainedForecastEvidenceReceipt};

use super::forecast_snapshot::{ForecastSnapshotCodec, ForecastSnapshotWatermarkEntry};

impl<Codec: ForecastEvidenceReceiptCodec> ForecastSnapshotCodec
    for ForecastEvidenceMaterial<Codec>
{
    /// read exact deterministic compressed objects through the retained material reader.
    fn admit_retained_object(
        &self,
        expected: ForecastObjectSha256,
        bytes: &[u8],
    ) -> Option<RetainedForecastEvidenceObject> {
        ForecastEvidenceMaterial::admit_retained_object(self, *expected.as_bytes(), bytes)
    }

    /// recover exact first-receipt values for stable snapshot pairing.
    fn admit_retained_receipt(
        &self,
        expected: ForecastObjectSha256,
        bytes: &[u8],
    ) -> Option<RetainedForecastEvidenceReceipt> {
        ForecastEvidenceMaterial::admit_retained_receipt(self, *expected.as_bytes(), bytes)
    }

    /// hash canonical ordered pairs without host paths or derived timestamps.
    fn watermark_sha256(
        &self,
        entries: &[ForecastSnapshotWatermarkEntry],
    ) -> Option<ForecastObjectSha256> {
        let pairs = JsonValue::Array(
            entries
                .iter()
                .map(
                    // retain only the two portable identities in the frozen receipt order
                    |entry| {
                        object([
                            (
                                "edgeReceiptIdentitySha256",
                                digest(entry.edge_receipt_identity_sha256),
                            ),
                            ("objectSha256", digest(entry.object_sha256)),
                        ])
                    },
                )
                .collect(),
        );
        Some(ForecastObjectSha256::admitted(
            self.codec().sha256(&self.codec().canonical_json(&pairs)?),
        ))
    }
}
