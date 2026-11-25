//! Bind permanent forecast-page gap records to their exact canonical Node documents.

use crate::ForecastEvidenceReceiptCodec;

use super::forecast_cycle_gap::{
    FORECAST_CYCLE_GAP_CONTRACT_VERSION, FORECAST_CYCLE_GAP_DISPOSITION, ForecastCycleGapCodec,
    ForecastCycleGapReason, ForecastCycleGapRecord,
};
use super::forecast_cycle_page_binding::{
    BoundForecastCyclePageCodec, digest, object, read_digest, read_field, read_generation,
    read_object, read_text, text,
};

impl<Codec: ForecastEvidenceReceiptCodec> ForecastCycleGapCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Reuse the source's closed categorical record and canonical newline framing.
    fn encode_cycle_gap(&self, record: &ForecastCycleGapRecord) -> Option<Vec<u8>> {
        self.frame(&object([
            ("contractVersion", text(FORECAST_CYCLE_GAP_CONTRACT_VERSION)),
            ("dueKey", text(&record.due_key)),
            ("generation", text(&record.generation.to_string())),
            ("occurredAt", text(&record.occurred_at)),
            (
                "projectionIdentitySha256",
                digest(record.projection_identity_sha256),
            ),
            (
                "qualificationDisposition",
                text(FORECAST_CYCLE_GAP_DISPOSITION),
            ),
            ("reason", text(record.reason.wire())),
        ]))
    }

    /// Confirm exact original byte identity before projecting typed gap metadata.
    fn decode_cycle_gap(&self, bytes: &[u8]) -> Option<ForecastCycleGapRecord> {
        let value = self.byte_codec().decode_retained_json(bytes)?;
        let fields = read_object(&value)?;
        // refuse foreign fields, lossy JSON framing and changes in qualification disposition
        if fields.len() != 7 || self.frame(&value)? != bytes {
            return None;
        }
        let version = read_text(read_field(fields, "contractVersion")?)?;
        let disposition = read_text(read_field(fields, "qualificationDisposition")?)?;
        // bind the original immutable gap classification
        if version != FORECAST_CYCLE_GAP_CONTRACT_VERSION
            || disposition != FORECAST_CYCLE_GAP_DISPOSITION
        {
            return None;
        }
        let reason = ForecastCycleGapReason::from_wire(&read_text(read_field(fields, "reason")?)?)?;
        Some(ForecastCycleGapRecord {
            due_key: read_text(read_field(fields, "dueKey")?)?,
            generation: read_generation(read_field(fields, "generation")?)?,
            projection_identity_sha256: read_digest(read_field(
                fields,
                "projectionIdentitySha256",
            )?)?,
            reason,
            occurred_at: read_text(read_field(fields, "occurredAt")?)?,
        })
    }
}
