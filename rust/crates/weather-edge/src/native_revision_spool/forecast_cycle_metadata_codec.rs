//! Narrow canonical metadata codec shared by legacy forecast-cycle records.

use weather_contracts::JsonValue;

use crate::ForecastEvidenceReceiptCodec;
use crate::archive_wire::{ArchiveWireCodec, ArchiveWirePrimitives};
use crate::revision_archive_canonical::RevisionArchiveCanonicalCodec;

use super::forecast_cycle_page_binding::BoundForecastCyclePageCodec;

/// JSON, SHA-256, and canonical UTC operations required by metadata-only cycle records.
pub trait ForecastCycleMetadataCodec: RevisionArchiveCanonicalCodec {
    /// Decode one retained JSON document without changing its scalar representation.
    fn decode_cycle_metadata(&self, bytes: &[u8]) -> Option<JsonValue>;

    /// Encode one recursively sorted canonical JSON document with its terminating LF.
    fn encode_cycle_metadata(&self, value: &JsonValue) -> Option<Vec<u8>>;
}

impl<P: ArchiveWirePrimitives> ForecastCycleMetadataCodec for ArchiveWireCodec<P> {
    /// Reuse the admitted archive JSON decoder without requiring forecast compression.
    fn decode_cycle_metadata(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.decode(bytes)
    }

    /// Reuse the admitted sorted archive frame without requiring forecast compression.
    fn encode_cycle_metadata(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.encode(value)
    }
}

impl<Codec: ForecastEvidenceReceiptCodec> ForecastCycleMetadataCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Preserve the existing retained JSON reader for the full forecast-cycle codec.
    fn decode_cycle_metadata(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.byte_codec().decode_retained_json(bytes)
    }

    /// Preserve the existing canonical no-LF encoding followed by exactly one LF.
    fn encode_cycle_metadata(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.frame(value)
    }
}
