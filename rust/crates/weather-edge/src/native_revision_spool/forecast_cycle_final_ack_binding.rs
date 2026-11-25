//! Bind the cold-graph acknowledgement to the deployed canonical JSON record.

use crate::ForecastEvidenceReceiptCodec;
use crate::archive_wire::{ArchiveWireCodec, ArchiveWirePrimitives};
use crate::revision_archive_canonical::instant;

use super::forecast_cycle_final_ack::{
    FORECAST_CYCLE_FINAL_ACK_CONTRACT_VERSION, ForecastCycleFinalAckCodec,
    ForecastCycleFinalAcknowledgement,
};
use super::forecast_cycle_metadata_codec::ForecastCycleMetadataCodec;
use super::forecast_cycle_page_binding::{
    BoundForecastCyclePageCodec, digest, object, read_digest, read_field, read_object, read_text,
    text,
};

impl<P: ArchiveWirePrimitives> ForecastCycleFinalAckCodec for ArchiveWireCodec<P> {
    /// Reuse the exact canonical fields and framing for the first cold-graph proof.
    fn encode_final_acknowledgement(
        &self,
        ack: &ForecastCycleFinalAcknowledgement,
    ) -> Option<Vec<u8>> {
        encode_final_acknowledgement(self, ack)
    }

    /// Decode only the exact canonical acknowledgement and its original UTC instant.
    fn decode_final_acknowledgement(
        &self,
        bytes: &[u8],
    ) -> Option<ForecastCycleFinalAcknowledgement> {
        decode_final_acknowledgement(self, bytes)
    }
}

impl<Codec: ForecastEvidenceReceiptCodec> ForecastCycleFinalAckCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Preserve exact full-codec encoding through the narrow shared metadata contract.
    fn encode_final_acknowledgement(
        &self,
        ack: &ForecastCycleFinalAcknowledgement,
    ) -> Option<Vec<u8>> {
        encode_final_acknowledgement(self, ack)
    }

    /// Preserve exact full-codec decoding through the narrow shared metadata contract.
    fn decode_final_acknowledgement(
        &self,
        bytes: &[u8],
    ) -> Option<ForecastCycleFinalAcknowledgement> {
        decode_final_acknowledgement(self, bytes)
    }
}

/// Encode the source final acknowledgement through one metadata-only owner.
fn encode_final_acknowledgement(
    codec: &impl ForecastCycleMetadataCodec,
    ack: &ForecastCycleFinalAcknowledgement,
) -> Option<Vec<u8>> {
    codec.encode_cycle_metadata(&object([
        ("acknowledgedAt", text(&ack.acknowledged_at)),
        ("contractVersion", text(ack.contract_version)),
        ("graphSha256", digest(ack.graph_sha256)),
        ("manifestSha256", digest(ack.manifest_sha256)),
    ]))
}

/// Decode the exact closed final acknowledgement through one metadata-only owner.
fn decode_final_acknowledgement(
    codec: &impl ForecastCycleMetadataCodec,
    bytes: &[u8],
) -> Option<ForecastCycleFinalAcknowledgement> {
    let value = codec.decode_cycle_metadata(bytes)?;
    let fields = read_object(&value)?;
    // preserve the closed source shape and complete original canonical framing
    if fields.len() != 4
        || read_text(read_field(fields, "contractVersion")?)?
            != FORECAST_CYCLE_FINAL_ACK_CONTRACT_VERSION
        || codec.encode_cycle_metadata(&value)? != bytes
    {
        return None;
    }
    let acknowledged_at = read_text(read_field(fields, "acknowledgedAt")?)?;
    instant(codec, &acknowledged_at)?;
    Some(ForecastCycleFinalAcknowledgement {
        acknowledged_at,
        contract_version: FORECAST_CYCLE_FINAL_ACK_CONTRACT_VERSION,
        graph_sha256: read_digest(read_field(fields, "graphSha256")?)?,
        manifest_sha256: read_digest(read_field(fields, "manifestSha256")?)?,
    })
}
