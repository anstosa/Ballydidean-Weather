//! Bind the edge archive command to the actual service-owned metadata proof writer.

use std::io::Write;
use std::path::PathBuf;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;
use weather_database::Sha256Digest;
use weather_edge::{
    NativeRevisionColdAckCodec, NativeRevisionSpool, RevisionArchiveCanonicalClock,
    RevisionColdAck, RevisionColdAckError, RevisionColdAckIdentity, RevisionColdMetadataProof,
    RevisionGapSha256, RevisionSpoolLeaseCodec, parse_revision_cold_ack_command,
};

use crate::{
    MaintenanceAnchorStatusJson, NativeShadowMetadataCustodyFiles,
    ShadowMetadataCustodyPublishInput, commit_shadow_metadata_custody_proof,
    prepare_shadow_metadata_custody_proof,
};

/// One exact evidence root and admitted operating-system service identity.
pub struct NativeColdCustodyRoot {
    pub root: PathBuf,
    pub uid: u32,
    pub gid: u32,
}

/// Selected canonical runtime primitives and clocks, independently borrowed.
pub struct NativeColdCustodyPorts<'a, ArchiveCodec, ProofCodec, Clock> {
    pub archive_codec: &'a ArchiveCodec,
    pub proof_codec: &'a mut ProofCodec,
    pub time: &'a dyn CurrentTimeResolver,
    pub clock: &'a mut Clock,
}

/// Dispatch the original custody command with the real native v2 metadata proof owner.
pub fn run_native_cold_custody_command<Lease, ArchiveCodec, ProofCodec, Clock>(
    action: &str,
    arguments: &[&str],
    root: NativeColdCustodyRoot,
    lease_codec: Lease,
    ports: NativeColdCustodyPorts<'_, ArchiveCodec, ProofCodec, Clock>,
    output: &mut impl Write,
) -> Result<bool, RevisionColdAckError>
where
    Lease: RevisionSpoolLeaseCodec,
    ArchiveCodec: NativeRevisionColdAckCodec,
    ProofCodec: MaintenanceAnchorStatusJson,
    Clock: RevisionArchiveCanonicalClock,
{
    use RevisionColdAckError as Error;
    // parse within the command owner before opening either actual storage root
    let Some(request) = parse_revision_cold_ack_command(action, arguments)? else {
        return Ok(false);
    };
    let files =
        NativeShadowMetadataCustodyFiles::for_evidence_root(root.root.clone(), root.uid, root.gid);
    let mut spool = NativeRevisionSpool::new(root.root, lease_codec).map_err(|_| Error::Storage)?;
    let mut proof = NativeColdMetadataProof {
        files,
        codec: ports.proof_codec,
        time: ports.time,
    };
    let bytes = spool.acknowledge_revision_cold_page(
        request,
        ports.archive_codec,
        ports.clock,
        Some(&mut proof),
    )?;
    output.write_all(&bytes).map_err(|_| Error::Storage)?;
    Ok(true)
}

/// Concrete metadata proof owner kept under the same evidence root as the native spool.
struct NativeColdMetadataProof<'a, Codec> {
    files: NativeShadowMetadataCustodyFiles,
    codec: &'a mut Codec,
    time: &'a dyn CurrentTimeResolver,
}

impl<Codec: MaintenanceAnchorStatusJson> RevisionColdMetadataProof
    for NativeColdMetadataProof<'_, Codec>
{
    /// Prepare the exact capsule-derived rows and ACK binding before publication.
    fn prepare_cold_metadata_proof(
        &mut self,
        entries: Vec<JsonValue>,
        acknowledgement: &RevisionColdAck,
    ) -> Result<(), RevisionColdAckError> {
        let RevisionColdAckIdentity::CustodyCheckpoint(checkpoint) =
            acknowledgement.request.identity
        else {
            return Err(RevisionColdAckError::MetadataProof);
        };
        let input = ShadowMetadataCustodyPublishInput {
            acknowledged_at: acknowledgement.acknowledged_at.clone(),
            acknowledgement_sha256: digest(acknowledgement.acknowledgement_sha256),
            archive_commit_ordinal: acknowledgement.next.ordinal,
            custody_checkpoint_sha256: digest(checkpoint),
            entries,
            frontier_sha256: digest(acknowledgement.next.frontier),
            member_root_sha256: digest(acknowledgement.request.member_root_sha256),
            page_sha256: digest(acknowledgement.request.page_sha256),
        };
        prepare_shadow_metadata_custody_proof(input, &mut self.files, self.codec, self.time)
            .map_err(|_| RevisionColdAckError::MetadataProof)?;
        Ok(())
    }

    /// Publish or reenter the real durable metadata proof under its acknowledged identity.
    fn commit_cold_metadata_proof(
        &mut self,
        acknowledgement_sha256: RevisionGapSha256,
    ) -> Result<(), RevisionColdAckError> {
        commit_shadow_metadata_custody_proof(
            &digest(acknowledgement_sha256),
            &mut self.files,
            self.codec,
            self.time,
        )
        .map_err(|_| RevisionColdAckError::MetadataProof)?;
        Ok(())
    }
}

/// Convert only the already admitted fixed SHA-256 representation between owners.
fn digest(value: RevisionGapSha256) -> Sha256Digest {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(64);
    // convert the fixed-size content address without performing another hash
    for byte in value.as_bytes() {
        write!(text, "{byte:02x}").expect("infallible string formatter");
    }
    Sha256Digest::admitted(text)
}
