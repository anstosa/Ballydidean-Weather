//! Read one committed scheduler capture into its canonical archive-page payload.

use std::path::{Path, PathBuf};

use weather_contracts::JsonValue;

use crate::native_revision_spool_file::read_private_file;
use crate::revision_archive_canonical::lower_hex;
use crate::{ForecastEvidenceMaterial, ForecastEvidenceReceiptCodec, PreparedForecastEvidence};

use super::forecast_channels::{ForecastChannelCodec, ForecastIssuanceChannel};
use super::forecast_objects::{
    FORECAST_OBJECT_MAXIMUM_BYTES, FORECAST_RECEIPT_MAXIMUM_BYTES, ForecastObjectSha256,
};

const PAYLOAD_CONTRACT: &str = "adjustment-evidence-page-payload/v1";

/// Exact read, integrity, or canonical encoding failure for one committed payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastSchedulerArchivePayloadError {
    AssertionInvalid,
    BindingInvalid,
    ChannelInvalid,
    Encoding,
    ObjectInvalid,
    ReceiptInvalid,
    Storage,
}

/// Read and bind one committed scheduler object graph into canonical newline-framed bytes.
pub fn read_committed_scheduler_payload<Codec: ForecastEvidenceReceiptCodec>(
    root: &Path,
    prepared: &PreparedForecastEvidence,
    material: &ForecastEvidenceMaterial<Codec>,
) -> Result<Vec<u8>, ForecastSchedulerArchivePayloadError> {
    use ForecastSchedulerArchivePayloadError as Error;

    let object_sha256 = ForecastObjectSha256::admitted(prepared.object_sha256());
    let edge_receipt_identity_sha256 =
        ForecastObjectSha256::admitted(prepared.edge_receipt_identity_sha256());
    let object_gzip_bytes = read_private_file(
        &member_path(root, "objects", object_sha256, ".json.gz"),
        FORECAST_OBJECT_MAXIMUM_BYTES,
    )
    .map_err(|_| Error::Storage)?;

    // bind the exact committed gzip bytes to the prepared object and retained validator
    if object_gzip_bytes != prepared.compressed_object()
        || material
            .admit_retained_object(prepared.object_sha256(), &object_gzip_bytes)
            .is_none()
    {
        return Err(Error::ObjectInvalid);
    }
    let receipt_bytes = read_private_file(
        &member_path(root, "receipts", edge_receipt_identity_sha256, ".json"),
        FORECAST_RECEIPT_MAXIMUM_BYTES,
    )
    .map_err(|_| Error::Storage)?;
    let receipt = material
        .admit_retained_receipt(prepared.edge_receipt_identity_sha256(), &receipt_bytes)
        .ok_or(Error::ReceiptInvalid)?;
    let receipt_value = material
        .codec()
        .decode_retained_json(&receipt_bytes)
        .ok_or(Error::ReceiptInvalid)?;
    let mut canonical_receipt = material
        .codec()
        .canonical_json(&receipt_value)
        .ok_or(Error::ReceiptInvalid)?;
    canonical_receipt.push(b'\n');

    // require the stable receipt to remain canonical and paired to this prepared object
    if receipt.object_sha256 != prepared.object_sha256() || canonical_receipt != receipt_bytes {
        return Err(Error::ReceiptInvalid);
    }
    let binding_bytes = read_private_file(
        &member_path(
            root,
            "channel-bindings",
            edge_receipt_identity_sha256,
            ".json",
        ),
        FORECAST_RECEIPT_MAXIMUM_BYTES,
    )
    .map_err(|_| Error::Storage)?;
    let binding = ForecastChannelCodec::decode_binding(
        material,
        edge_receipt_identity_sha256,
        &binding_bytes,
    )
    .ok_or(Error::BindingInvalid)?;
    let assertion_bytes = read_private_file(
        &member_path(root, "channels", binding.assertion_sha256, ".json"),
        FORECAST_RECEIPT_MAXIMUM_BYTES,
    )
    .map_err(|_| Error::Storage)?;
    let assertion = ForecastChannelCodec::decode_assertion(
        material,
        binding.assertion_sha256,
        &assertion_bytes,
    )
    .ok_or(Error::AssertionInvalid)?;
    let receipt_sha256 = ForecastChannelCodec::receipt_sha256(material, &receipt_bytes);

    // archive only the assertion selected by the trusted scheduler request map
    if assertion.channel != ForecastIssuanceChannel::SchedulerRequest
        || assertion.receipt_sha256 != receipt_sha256
    {
        return Err(Error::ChannelInvalid);
    }
    encode_payload(
        material.codec(),
        prepared,
        binding.assertion_sha256,
        &assertion_bytes,
        &binding_bytes,
        &object_gzip_bytes,
        &receipt_bytes,
    )
}

// derive one exact immutable member path from its admitted content address
fn member_path(
    root: &Path,
    directory: &str,
    identity_sha256: ForecastObjectSha256,
    suffix: &str,
) -> PathBuf {
    root.join(directory).join(format!(
        "sha256-{}{}",
        lower_hex(*identity_sha256.as_bytes()),
        suffix
    ))
}

// encode the exact eleven-field Node page payload through the approved codec owner
fn encode_payload(
    codec: &impl ForecastEvidenceReceiptCodec,
    prepared: &PreparedForecastEvidence,
    assertion_sha256: ForecastObjectSha256,
    assertion_bytes: &[u8],
    binding_bytes: &[u8],
    object_gzip_bytes: &[u8],
    receipt_bytes: &[u8],
) -> Result<Vec<u8>, ForecastSchedulerArchivePayloadError> {
    use ForecastSchedulerArchivePayloadError as Error;

    let value = object([
        (
            "channelAssertionBase64",
            string(
                &codec
                    .standard_base64(assertion_bytes)
                    .ok_or(Error::Encoding)?,
            ),
        ),
        (
            "channelAssertionSha256",
            string(&lower_hex(*assertion_sha256.as_bytes())),
        ),
        (
            "channelBindingBase64",
            string(
                &codec
                    .standard_base64(binding_bytes)
                    .ok_or(Error::Encoding)?,
            ),
        ),
        (
            "channelBindingSha256",
            string(&lower_hex(codec.sha256(binding_bytes))),
        ),
        ("contractVersion", string(PAYLOAD_CONTRACT)),
        (
            "edgeReceiptIdentitySha256",
            string(&lower_hex(prepared.edge_receipt_identity_sha256())),
        ),
        (
            "objectGzipBase64",
            string(
                &codec
                    .standard_base64(object_gzip_bytes)
                    .ok_or(Error::Encoding)?,
            ),
        ),
        (
            "objectGzipSha256",
            string(&lower_hex(codec.sha256(object_gzip_bytes))),
        ),
        ("objectSha256", string(&lower_hex(prepared.object_sha256()))),
        (
            "receiptBase64",
            string(
                &codec
                    .standard_base64(receipt_bytes)
                    .ok_or(Error::Encoding)?,
            ),
        ),
        (
            "receiptSha256",
            string(&lower_hex(codec.sha256(receipt_bytes))),
        ),
    ]);
    let mut canonical = codec.canonical_json(&value).ok_or(Error::Encoding)?;
    canonical.push(b'\n');
    Ok(canonical)
}

// construct one closed typed object for the existing canonical JSON owner
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(
                // retain the exact source field spelling for canonical ordering
                |(name, value)| (name.encode_utf16().collect(), value),
            )
            .collect(),
    )
}

// construct one typed string without adding a JSON implementation
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
