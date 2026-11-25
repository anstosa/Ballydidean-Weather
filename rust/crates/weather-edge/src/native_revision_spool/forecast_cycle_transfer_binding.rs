//! Bind exact retained transfer checkpoints to the existing canonical codec owner.

use weather_contracts::JsonValue;

use crate::ForecastEvidenceReceiptCodec;
use crate::archive_wire::{ArchiveWireCodec, ArchiveWirePrimitives};
use crate::forecast_cycle_pages::{
    FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS, FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES,
};
use crate::revision_archive_canonical::instant;

use super::forecast_cycle_metadata_codec::ForecastCycleMetadataCodec;
use super::forecast_cycle_page_binding::{
    BoundForecastCyclePageCodec, digest, object, page_header, projection, read_digest, read_field,
    read_number, read_object, read_page_header, read_projection, read_text, text,
};
use super::forecast_cycle_transfer_checkpoint::{
    FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION, ForecastCycleTransferCheckpoint,
    ForecastCycleTransferCheckpointCodec,
};

impl<P: ArchiveWirePrimitives> ForecastCycleTransferCheckpointCodec for ArchiveWireCodec<P> {
    /// Persist the existing immutable transfer witness without retaining any page bytes.
    fn encode_transfer_checkpoint(
        &self,
        checkpoint: &ForecastCycleTransferCheckpoint,
    ) -> Option<Vec<u8>> {
        encode_transfer_checkpoint(self, checkpoint)
    }

    /// Recover the closed canonical metadata and original projection sequence.
    fn decode_transfer_checkpoint(&self, bytes: &[u8]) -> Option<ForecastCycleTransferCheckpoint> {
        decode_transfer_checkpoint(self, bytes)
    }
}

impl<Codec: ForecastEvidenceReceiptCodec> ForecastCycleTransferCheckpointCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Preserve exact full-codec encoding through the narrow shared metadata contract.
    fn encode_transfer_checkpoint(
        &self,
        checkpoint: &ForecastCycleTransferCheckpoint,
    ) -> Option<Vec<u8>> {
        encode_transfer_checkpoint(self, checkpoint)
    }

    /// Preserve exact full-codec decoding through the narrow shared metadata contract.
    fn decode_transfer_checkpoint(&self, bytes: &[u8]) -> Option<ForecastCycleTransferCheckpoint> {
        decode_transfer_checkpoint(self, bytes)
    }
}

/// Encode one exact transfer checkpoint through a metadata-only owner.
fn encode_transfer_checkpoint(
    codec: &impl ForecastCycleMetadataCodec,
    checkpoint: &ForecastCycleTransferCheckpoint,
) -> Option<Vec<u8>> {
    let payload_length: u32 = checkpoint.payload_length.try_into().ok()?;
    codec.encode_cycle_metadata(&object([
        ("acknowledgedAt", text(&checkpoint.acknowledged_at)),
        ("checkpointSha256", digest(checkpoint.checkpoint_sha256)),
        ("contractVersion", text(checkpoint.contract_version)),
        ("header", page_header(&checkpoint.header)?),
        ("pageSha256", digest(checkpoint.page_sha256)),
        (
            "payloadLength",
            JsonValue::Number(f64::from(payload_length)),
        ),
        (
            "projections",
            JsonValue::Array(checkpoint.projections.iter().map(projection).collect()),
        ),
    ]))
}

/// Decode one exact transfer checkpoint through a metadata-only owner.
fn decode_transfer_checkpoint(
    codec: &impl ForecastCycleMetadataCodec,
    bytes: &[u8],
) -> Option<ForecastCycleTransferCheckpoint> {
    let value = codec.decode_cycle_metadata(bytes)?;
    let fields = read_object(&value)?;
    // require the exact source fields, version and original canonical byte identity
    if fields.len() != 7
        || read_text(read_field(fields, "contractVersion")?)?
            != FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION
        || codec.encode_cycle_metadata(&value)? != bytes
    {
        return None;
    }
    let acknowledged_at = read_text(read_field(fields, "acknowledgedAt")?)?;
    let payload_length: usize = read_number(read_field(fields, "payloadLength")?)?
        .try_into()
        .ok()?;
    let JsonValue::Array(projections) = read_field(fields, "projections")? else {
        return None;
    };
    // preserve finite source bounds and the exact admitted acknowledgement instant
    if instant(codec, &acknowledged_at).is_none()
        || !(1..=FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES).contains(&payload_length)
        || !(1..=FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS).contains(&projections.len())
    {
        return None;
    }
    Some(ForecastCycleTransferCheckpoint {
        acknowledged_at,
        checkpoint_sha256: read_digest(read_field(fields, "checkpointSha256")?)?,
        contract_version: FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION,
        header: read_page_header(read_field(fields, "header")?)?,
        page_sha256: read_digest(read_field(fields, "pageSha256")?)?,
        payload_length,
        projections: projections
            .iter()
            .map(read_projection)
            .collect::<Option<Vec<_>>>()?,
    })
}
