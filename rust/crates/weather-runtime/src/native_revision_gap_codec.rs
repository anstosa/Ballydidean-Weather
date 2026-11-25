//! Bind the native retained gap transfer to the approved archive wire owner.

use weather_contracts::{
    AdjustmentRevisionProjectionKind, JsonValue, RainMaintenanceControlStateStageReceipt,
};
use weather_edge::{
    AdjustmentMaintenanceGapReceipt, AdjustmentRevisionGapReceipt, AdmittedRevisionGapAckIdentity,
    AdmittedRevisionGapPageIdentity, ArchiveWireCodec, NativeRevisionGapAckDecoder,
    NativeRevisionGapCommandCodec, NativeRevisionGapMember, NativeRevisionGapPayloadKind,
    NativeRevisionGapProjection, NativeRevisionGapShadowStage, NativeRevisionGapTransferCodec,
    NativeRevisionSpoolArtifactCodec, PermanentArchiveGapCodec, PermanentArchiveGapInput,
    RainControlStateWireCodec, RainMaintenanceControlStateGapReceipt, RetainedRevisionGapEntry,
    RevisionArchiveCanonicalCodec, RevisionGapAckCodec, RevisionGapFrontier,
    RevisionGapIdentityAdapter, RevisionGapPayloadAcknowledgement, RevisionGapPayloadPage,
    RevisionGapRetirementGroup, RevisionGapSha256, RevisionGapTransferError,
    RevisionGapTransferStart, RevisionGapUnsignedAcknowledgement, RevisionGapUnsignedPage,
    RevisionGapUnsignedStart,
};

use crate::{
    NativeWirePrimitives,
    native_revision_gap_artifacts::{NativeRevisionGapArtifactCodec, admitted_gap_rain_state},
};
use crate::{native_revision_gap_decode as decode, native_revision_gap_identity as identity};

/// One stateless production owner for the original permanent-gap command wire.
#[derive(Clone, Copy, Default)]
pub struct NativeRevisionGapCodec;

impl NativeRevisionGapCodec {
    /// Reuse the existing zero-state archive contract owner for typed retained artifacts.
    const fn wire() -> ArchiveWireCodec<NativeWirePrimitives> {
        ArchiveWireCodec::new(NativeWirePrimitives)
    }
}

impl RevisionArchiveCanonicalCodec for NativeRevisionGapCodec {
    /// Admit the exact original UTC millisecond spelling.
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        NativeWirePrimitives.canonical_utc_milliseconds(text)
    }

    /// Hash the exact selected filesystem bytes.
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives, bytes)
    }
}

impl RainControlStateWireCodec for NativeRevisionGapCodec {
    /// Delegate monthly control-state JSON parsing to its existing owner.
    fn decode_control_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        Self::wire().decode_control_json(bytes)
    }

    /// Preserve producer-ordered control-state JSON bytes.
    fn encode_control_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        Self::wire().encode_control_ordered_json(value)
    }

    /// Preserve the recursively sorted self-address preimage.
    fn encode_control_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        Self::wire().encode_control_sorted_json(value)
    }

    /// Decode the exact retained monthly stage receipt.
    fn decode_stage_receipt(
        &self,
        bytes: &[u8],
    ) -> Option<RainMaintenanceControlStateStageReceipt> {
        RainControlStateWireCodec::decode_stage_receipt(&Self::wire(), bytes)
    }

    /// Decode the exact retained monthly abandonment receipt.
    fn decode_gap_receipt(&self, bytes: &[u8]) -> Option<RainMaintenanceControlStateGapReceipt> {
        Self::wire().decode_gap_receipt(bytes)
    }
}

impl PermanentArchiveGapCodec for NativeRevisionGapCodec {
    /// Reuse the signed and unsigned locator grammar from the archive wire.
    fn encode_gap_key(&self, input: PermanentArchiveGapInput<'_>) -> Option<Vec<u8>> {
        Self::wire().encode_gap_key(input)
    }

    /// Preserve the original permanent-gap envelope canonical identity.
    fn encode_gap_receipt(
        &self,
        input: PermanentArchiveGapInput<'_>,
        recorded_at: &str,
        digest: Option<&str>,
    ) -> Option<Vec<u8>> {
        Self::wire().encode_gap_receipt(input, recorded_at, digest)
    }

    /// Admit the complete revision disposition through the original typed codec.
    fn accepted_revision_gap(&self, bytes: &[u8]) -> Option<AdjustmentRevisionGapReceipt> {
        Self::wire().accepted_revision_gap(bytes)
    }

    /// Admit the complete shadow disposition through the original typed codec.
    fn accepted_shadow_gap(&self, bytes: &[u8]) -> Option<AdjustmentMaintenanceGapReceipt> {
        Self::wire().accepted_shadow_gap(bytes)
    }
}

impl NativeRevisionSpoolArtifactCodec for NativeRevisionGapCodec {
    /// Resolve the exact ordinal artifact only after native control-state admission.
    fn admitted_rain_control_ordinal_sha256(&self, bytes: &[u8]) -> Option<String> {
        Self::wire().admitted_rain_control_ordinal_sha256(bytes)
    }
}

impl RevisionGapIdentityAdapter<NativeRevisionGapMember> for NativeRevisionGapCodec {
    /// Derive the original start identity from the canonical unsigned envelope.
    fn start_sha256(
        &mut self,
        start: RevisionGapUnsignedStart,
    ) -> Result<RevisionGapSha256, RevisionGapTransferError> {
        identity::start_sha256(&NativeWirePrimitives, start)
            .ok_or(RevisionGapTransferError::IdentityUnavailable)
    }

    /// Derive the exact member root, page identity and encoded byte count.
    fn page_identity(
        &mut self,
        page: RevisionGapUnsignedPage<'_, NativeRevisionGapMember>,
    ) -> Result<AdmittedRevisionGapPageIdentity, RevisionGapTransferError> {
        identity::page_identity(&NativeWirePrimitives, page)
            .ok_or(RevisionGapTransferError::IdentityUnavailable)
    }
}

impl NativeRevisionGapTransferCodec for NativeRevisionGapCodec {
    /// Observe only the fully canonical acknowledged frontier.
    fn admitted_frontier(&self, bytes: &[u8]) -> Option<RevisionGapFrontier> {
        let ack = self.admitted_acknowledgement(bytes)?;
        Some(RevisionGapFrontier {
            frontier_sha256: ack.frontier_sha256,
            page_count: ack.page_count,
        })
    }

    /// Apply the source retained-state grammar without revalidating a new producer write.
    fn admitted_gap_rain_state(&self, bytes: &[u8], expected_ordinal: Option<&str>) -> bool {
        admitted_gap_rain_state(&NativeWirePrimitives, bytes, expected_ordinal)
    }

    /// Reconstruct the exact native pending page from its retained bytes.
    fn admitted_pending(
        &self,
        bytes: &[u8],
    ) -> Option<RevisionGapPayloadPage<NativeRevisionGapMember>> {
        self.admitted_pending_page(bytes)
    }

    /// Emit the complete admitted native page using the original field names.
    fn encode_page(
        &self,
        page: &RevisionGapPayloadPage<NativeRevisionGapMember>,
    ) -> Option<Vec<u8>> {
        identity::encode_page(&NativeWirePrimitives, page)
    }

    /// Bind a retained single commit receipt to its addressed frontier.
    fn single_commit_projection(
        &self,
        bytes: &[u8],
        frontier_sha256: RevisionGapSha256,
    ) -> Option<RevisionGapSha256> {
        NativeRevisionGapArtifactCodec::native().single_commit_projection(bytes, frontier_sha256)
    }

    /// Bind a complete retained commit group to its batch projection address.
    fn accepted_commit_group(&self, bytes: &[u8], projection_sha256: RevisionGapSha256) -> bool {
        NativeRevisionGapArtifactCodec::native().accepted_commit_group(bytes, projection_sha256)
    }

    /// Decode projected payload classification using the shared producer validator.
    fn gap_projection(&self, bytes: &[u8]) -> Option<NativeRevisionGapProjection> {
        NativeRevisionGapArtifactCodec::native()
            .gap_projection(bytes)
            .filter(|projection| {
                projection.kind != NativeRevisionGapPayloadKind::TemperatureNativeSourceProjection
            })
    }

    /// Admit exact source kind and stage receipt identity.
    fn accepted_projection_stage(
        &self,
        bytes: &[u8],
        slot: RevisionGapSha256,
        kind: AdjustmentRevisionProjectionKind,
    ) -> bool {
        NativeRevisionGapArtifactCodec::native().accepted_projection_stage(bytes, slot, kind)
    }

    /// Decode a retained scheduled shadow stage and bound receipt bytes.
    fn gap_shadow_stage(&self, bytes: &[u8]) -> Option<NativeRevisionGapShadowStage> {
        NativeRevisionGapArtifactCodec::native().gap_shadow_stage(bytes)
    }
}

impl NativeRevisionGapAckDecoder for NativeRevisionGapCodec {
    /// Decode and validate the full canonical acknowledgement.
    fn admitted_acknowledgement(&self, bytes: &[u8]) -> Option<RevisionGapPayloadAcknowledgement> {
        decode::decode_acknowledgement(&NativeWirePrimitives, bytes)
    }

    /// Decode and validate the full frozen pending-page member set.
    fn admitted_pending_page(
        &self,
        bytes: &[u8],
    ) -> Option<RevisionGapPayloadPage<NativeRevisionGapMember>> {
        decode::decode_page(&NativeWirePrimitives, bytes)
    }

    /// Hash exact retained file bytes through the shared owner.
    fn storage_sha256(&self, bytes: &[u8]) -> [u8; 32] {
        self.sha256(bytes)
    }
}

impl RevisionGapAckCodec<NativeRevisionGapMember> for NativeRevisionGapCodec {
    /// Derive the fixed empty frontier identity.
    fn genesis_frontier_sha256(&self) -> RevisionGapSha256 {
        identity::genesis_frontier_sha256(&NativeWirePrimitives)
    }

    /// Serialize the exact pending payload page.
    fn payload_page_bytes(
        &self,
        page: &RevisionGapPayloadPage<NativeRevisionGapMember>,
    ) -> Option<Vec<u8>> {
        identity::encode_page(&NativeWirePrimitives, page)
    }

    /// Derive the compact checkpoint and acknowledgement identities.
    fn acknowledgement_identity(
        &self,
        value: RevisionGapUnsignedAcknowledgement<'_>,
    ) -> Option<AdmittedRevisionGapAckIdentity> {
        identity::acknowledgement_identity(&NativeWirePrimitives, value)
    }

    /// Emit one fully bound canonical acknowledgement envelope.
    fn acknowledgement_bytes(&self, value: &RevisionGapPayloadAcknowledgement) -> Option<Vec<u8>> {
        identity::encode_acknowledgement(&NativeWirePrimitives, value)
    }

    /// Reuse the existing typed retirement-group projection without constructing paths.
    fn retirement_groups(
        &self,
        entry: &RetainedRevisionGapEntry<NativeRevisionGapMember>,
    ) -> Option<Vec<RevisionGapRetirementGroup>> {
        entry.value.retirement_groups()
    }
}

impl NativeRevisionGapCommandCodec for NativeRevisionGapCodec {
    /// Emit one canonical transfer-start document.
    fn encode_start(&self, start: &RevisionGapTransferStart) -> Option<Vec<u8>> {
        identity::encode_start(&NativeWirePrimitives, start)
    }
}
