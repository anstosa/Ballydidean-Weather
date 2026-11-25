//! Native successful cold-page acknowledgement under the shared spool lease.

use std::io::Write;

use super::{NativeRevisionColdPageCodec, NativeRevisionSpool, RevisionSpoolLeaseCodec};
use crate::revision_archive_canonical::{RevisionArchiveCanonicalClock, instant};
use crate::revision_cold_ack::{
    RevisionColdAck, RevisionColdAckError, RevisionColdAckRequest, RevisionColdMetadataProof,
    cold_ack_retry, parse_revision_cold_ack_command, verify_cold_ack_page,
};
use crate::revision_cold_ack_codec::{
    RevisionColdAckJsonCodec, decode_revision_cold_ack, encode_revision_cold_ack,
};
use crate::revision_cold_ack_material::{
    RevisionColdAckMaterialCodec, derive_revision_cold_ack_page,
};
use crate::revision_gap_transfer::RevisionGapSha256;

/// Admitted canonical archive primitives required by either original custody verb.
pub trait NativeRevisionColdAckCodec:
    NativeRevisionColdPageCodec + RevisionColdAckJsonCodec + RevisionColdAckMaterialCodec
{
}

impl<Codec> NativeRevisionColdAckCodec for Codec where
    Codec: NativeRevisionColdPageCodec + RevisionColdAckJsonCodec + RevisionColdAckMaterialCodec
{
}

impl<Lease: RevisionSpoolLeaseCodec> NativeRevisionSpool<Lease> {
    /// Execute the versioned original successful custody transaction, or resume its exact retry.
    pub fn acknowledge_revision_cold_page(
        &mut self,
        request: RevisionColdAckRequest,
        codec: &impl NativeRevisionColdAckCodec,
        time: &mut impl RevisionArchiveCanonicalClock,
        proof: Option<&mut dyn RevisionColdMetadataProof>,
    ) -> Result<Vec<u8>, RevisionColdAckError> {
        self.acknowledge_revision_cold_page_with_seam(request, codec, time, proof, |_| Ok(()))
    }

    /// Preserve the predecessor's deterministic interruption boundary after durable publication.
    pub fn acknowledge_revision_cold_page_with_seam(
        &mut self,
        request: RevisionColdAckRequest,
        codec: &impl NativeRevisionColdAckCodec,
        time: &mut impl RevisionArchiveCanonicalClock,
        mut proof: Option<&mut dyn RevisionColdMetadataProof>,
        after_durable: impl FnOnce(&RevisionColdAck) -> Result<(), RevisionColdAckError>,
    ) -> Result<Vec<u8>, RevisionColdAckError> {
        use RevisionColdAckError as Error;
        // a v2 request must have its actual metadata proof owner before touching custody
        if request.identity.packed() && proof.is_none() {
            return Err(Error::MetadataProof);
        }
        self.require_cold_ack_directories()?;
        self.with_lease(
            |spool| {
                spool.require_cold_ack_census()?;
                let current = spool.read_cold_ack_state(
                    // decode the exact bounded retained pair, not a caller-supplied checkpoint
                    |bytes| decode_revision_cold_ack(bytes, codec),
                )?;
                let current_ack = current.as_ref().map(|(ack, _)| ack);

                // recover the first already durable ACK without requiring retired page members
                if cold_ack_retry(request, current_ack)? {
                    let (ack, bytes) = current.ok_or(Error::State)?;
                    // complete a packed metadata proof before retrying the exact retirement set
                    if request.identity.packed() {
                        proof
                            .as_deref_mut()
                            .ok_or(Error::MetadataProof)?
                            .commit_cold_metadata_proof(ack.acknowledgement_sha256)?;
                    }
                    spool.retire_cold_ack_entries(&ack.retirement_entries, codec)?;
                    return Ok(bytes);
                }

                let page_bytes = spool
                    .read_revision_cold_page(request.page, codec)
                    .map_err(|_| Error::Page)?;
                let page =
                    derive_revision_cold_ack_page(&page_bytes, request.start_member_sha256, codec)?;
                verify_cold_ack_page(request, &page, current_ack, codec)?;
                let acknowledged_at = time.canonical_now().ok_or(Error::Input)?;
                // bind the acknowledgement time to the already admitted UTC owner
                if instant(codec, &acknowledged_at).is_none() {
                    return Err(Error::Input);
                }
                let mut acknowledgement = RevisionColdAck {
                    acknowledged_at,
                    request,
                    next: page.next,
                    previous_acknowledgement_sha256: current_ack
                        .map(|ack| ack.acknowledgement_sha256),
                    acknowledgement_sha256: RevisionGapSha256::admitted([0; 32]),
                    retirement_entries: page.retirement_entries,
                };
                let unsigned = encode_revision_cold_ack(&acknowledgement, codec, false)
                    .ok_or(Error::Encoding)?;
                acknowledgement.acknowledgement_sha256 =
                    RevisionGapSha256::admitted(codec.sha256(&unsigned));
                let bytes = encode_revision_cold_ack(&acknowledgement, codec, true)
                    .ok_or(Error::Encoding)?;
                // admit the complete canonical checkpoint before changing any archive files
                if decode_revision_cold_ack(&bytes, codec).as_ref() != Some(&acknowledgement) {
                    return Err(Error::Encoding);
                }
                // prepare capsule-derived metadata before making the packed ACK durable
                if request.identity.packed() {
                    proof
                        .as_deref_mut()
                        .ok_or(Error::MetadataProof)?
                        .prepare_cold_metadata_proof(
                            page.packed_shadow_entries.ok_or(Error::MetadataProof)?,
                            &acknowledgement,
                        )?;
                }
                spool.publish_cold_ack(
                    current.as_ref().map(|(_, bytes)| bytes.as_slice()),
                    &bytes,
                )?;
                // expose packed proof current only after publication succeeds
                if request.identity.packed() {
                    proof
                        .ok_or(Error::MetadataProof)?
                        .commit_cold_metadata_proof(acknowledgement.acknowledgement_sha256)?;
                }
                after_durable(&acknowledgement)?;
                spool.retire_cold_ack_entries(&acknowledgement.retirement_entries, codec)?;
                Ok(bytes)
            },
            Error::Storage,
        )
    }

    /// Dispatch either original custody verb and emit one canonical durable acknowledgement.
    pub fn run_revision_cold_ack_command(
        &mut self,
        action: &str,
        arguments: &[&str],
        codec: &impl NativeRevisionColdAckCodec,
        time: &mut impl RevisionArchiveCanonicalClock,
        proof: Option<&mut dyn RevisionColdMetadataProof>,
        output: &mut impl Write,
    ) -> Result<bool, RevisionColdAckError> {
        let Some(request) = parse_revision_cold_ack_command(action, arguments)? else {
            return Ok(false);
        };
        let acknowledgement = self.acknowledge_revision_cold_page(request, codec, time, proof)?;
        output
            .write_all(&acknowledgement)
            .map_err(|_| RevisionColdAckError::Storage)?;
        Ok(true)
    }
}
