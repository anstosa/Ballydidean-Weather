//! Project exact page ranges from an admitted immutable manifest for cold storage.

use weather_contracts::JsonValue;

use crate::ForecastEvidenceReceiptCodec;

use super::forecast_cycle_capsule::{
    ForecastCycleCapsuleCodec, ForecastCycleCapsuleManifestLayout, ForecastCycleCapsulePageRange,
};
use super::forecast_cycle_page_binding::{
    BoundForecastCyclePageCodec, read_digest, read_field, read_number, read_object,
};

impl<Codec: ForecastEvidenceReceiptCodec> ForecastCycleCapsuleCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Bind only the ordered lengths and digest already admitted by the final manifest codec.
    fn capsule_manifest_layout(
        &self,
        manifest: &Self::Manifest,
    ) -> Option<ForecastCycleCapsuleManifestLayout> {
        let fields = read_object(manifest.document())?;
        let JsonValue::Array(values) = read_field(fields, "pages")? else {
            return None;
        };
        let pages = values
            .iter()
            .map(|value| {
                let fields = read_object(value)?;
                Some(ForecastCycleCapsulePageRange {
                    payload_offset: read_number(read_field(fields, "payloadOffset")?)?,
                    payload_length: read_number(read_field(fields, "payloadLength")?)?
                        .try_into()
                        .ok()?,
                    payload_sha256: read_digest(read_field(fields, "payloadSha256")?)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(ForecastCycleCapsuleManifestLayout {
            pages,
            total_payload_bytes: read_number(read_field(fields, "totalPayloadBytes")?)?,
        })
    }
}
