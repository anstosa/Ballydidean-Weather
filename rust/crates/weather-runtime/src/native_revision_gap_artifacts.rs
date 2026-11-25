//! Native gap-transfer artifact admission over the retained archive codecs.

use weather_contracts::AdjustmentRevisionProjectionKind;
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, NativeRevisionColdGroupCodec,
    NativeRevisionGapPayloadKind, NativeRevisionGapProjection, NativeRevisionGapRainBinding,
    NativeRevisionGapShadowStage, PrivateRevisionReceiptCodec, PrivateShadowArchiveVersion,
    PrivateShadowReceiptCodec, RevisionGapSha256, shadow_stage_receipt_value,
};

use crate::{NativeWirePrimitives, maintenance_canonical_json::text};

/// Pure native admission for the five retained artifact classes used by gap collection.
pub(crate) struct NativeRevisionGapArtifactCodec<P = NativeWirePrimitives> {
    wire: ArchiveWireCodec<P>,
}

impl NativeRevisionGapArtifactCodec<NativeWirePrimitives> {
    /// Construct the production artifact codec with the approved zero-state primitives.
    pub(crate) const fn native() -> Self {
        Self::new(NativeWirePrimitives::new())
    }
}

impl<P> NativeRevisionGapArtifactCodec<P>
where
    P: ArchiveWirePrimitives,
{
    /// Bind one primitive owner to the retained archive validators.
    pub(crate) const fn new(primitives: P) -> Self {
        Self {
            wire: ArchiveWireCodec::new(primitives),
        }
    }

    /// Admit one canonical commit receipt at its exact frontier address.
    pub(crate) fn single_commit_projection(
        &self,
        bytes: &[u8],
        frontier_sha256: RevisionGapSha256,
    ) -> Option<RevisionGapSha256> {
        let receipt = self.wire.admitted_commit_receipt(bytes)?;
        (receipt.frontier_sha256 == frontier_sha256).then_some(receipt.projection_identity_sha256)
    }

    /// Admit one complete canonical grouped chain at its projection address.
    pub(crate) fn accepted_commit_group(
        &self,
        bytes: &[u8],
        projection_sha256: RevisionGapSha256,
    ) -> bool {
        self.wire.admitted_batch_commit_group(bytes).is_some_and(
            // bind the complete admitted group to its batch filename
            |group| group.projection_identity_sha256 == projection_sha256,
        )
    }

    /// Read only the staged-body discriminator and optional control binding used by the predecessor collector.
    pub(crate) fn gap_projection(&self, bytes: &[u8]) -> Option<NativeRevisionGapProjection> {
        let value = self.wire.primitives().decode_json(bytes)?;
        // preserve the retained gap grammar without granting fresh producer-write admission
        let (kind, rain_control) = match text(&value, "contractVersion")?.as_str() {
            "adjustment-revision-projection/v1" => (NativeRevisionGapPayloadKind::Projection, None),
            "adjustment-revision-batch-projection/v2" => {
                (NativeRevisionGapPayloadKind::BatchProjection, None)
            }
            "adjustment-rain-gate-feature-projection/v2" => {
                (NativeRevisionGapPayloadKind::RainGateFeature, None)
            }
            "adjustment-rain-fixed-gauge-target-projection/v1" => {
                (NativeRevisionGapPayloadKind::RainFixedGaugeProjection, None)
            }
            "adjustment-temperature-native-source-projection/v2" => (
                NativeRevisionGapPayloadKind::TemperatureNativeSourceProjection,
                None,
            ),
            "adjustment-rain-gate-control-projection/v3" => {
                let state_sha256 =
                    RevisionGapSha256::from_lower_hex(&text(&value, "stateSha256")?)?;
                let state_stage_receipt_sha256 = text(&value, "stateStageReceiptSha256")?;
                RevisionGapSha256::from_lower_hex(&state_stage_receipt_sha256)?;
                (
                    NativeRevisionGapPayloadKind::RainGateControl,
                    Some(NativeRevisionGapRainBinding {
                        state_sha256,
                        ordinal_artifact_sha256: text(&value, "ordinalArtifactSha256")?,
                        state_stage_receipt_sha256,
                    }),
                )
            }
            _ => return None,
        };
        Some(NativeRevisionGapProjection { kind, rain_control })
    }

    /// Admit one canonical stage receipt bound to both names of the staged body.
    pub(crate) fn accepted_projection_stage(
        &self,
        bytes: &[u8],
        slot: RevisionGapSha256,
        kind: AdjustmentRevisionProjectionKind,
    ) -> bool {
        // fail closed before comparing the requested typed address
        let Some(receipt) = self.wire.decode_stage_receipt(bytes) else {
            return false;
        };
        RevisionGapSha256::from_lower_hex(&receipt.projectionIdentitySha256) == Some(slot)
            && RevisionGapSha256::from_lower_hex(&receipt.projectionSha256) == Some(slot)
            && receipt.projectionKind == kind
    }

    /// Admit one canonical shadow stage and retain its exact embedded receipt bytes.
    pub(crate) fn gap_shadow_stage(&self, bytes: &[u8]) -> Option<NativeRevisionGapShadowStage> {
        let document = self.wire.decode_stage_document(bytes)?;
        // preserve the document grammar in the exported payload kind
        let kind = match document.version {
            PrivateShadowArchiveVersion::V1 => NativeRevisionGapPayloadKind::ShadowStageV1,
            PrivateShadowArchiveVersion::V2 => NativeRevisionGapPayloadKind::ShadowStageV2,
        };
        let stage_receipt_bytes = self
            .wire
            .primitives()
            .encode_sorted_json(&shadow_stage_receipt_value(&document.stage_receipt))?;
        Some(NativeRevisionGapShadowStage {
            kind,
            due_key: document.input.metadata.dueKey,
            registration_sha256: document.input.metadata.registrationSha256,
            prediction_sha256: RevisionGapSha256::from_lower_hex(
                &document.input.metadata.predictionSha256,
            )?,
            stage_receipt_bytes,
        })
    }
}

/// Admit the predecessor retained-state envelope, optionally binding its ordinal to a control projection.
pub(crate) fn admitted_gap_rain_state(
    primitives: &impl ArchiveWirePrimitives,
    bytes: &[u8],
    expected_ordinal: Option<&str>,
) -> bool {
    // parse the actual canonical state and not its fresh producer-side domain grammar
    let Some(value) = primitives.decode_json(bytes) else {
        return false;
    };
    text(&value, "contractVersion").as_deref() == Some("rain-maintenance-control-state/v1")
        && primitives.encode_sorted_json(&value).as_deref() == Some(bytes)
        && expected_ordinal
            .is_none_or(|ordinal| text(&value, "ordinalArtifactSha256").as_deref() == Some(ordinal))
}
