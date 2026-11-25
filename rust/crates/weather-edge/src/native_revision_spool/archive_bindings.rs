//! Concrete revision/shadow file locations and durable operation bindings.

use std::fs;
use std::io;

use weather_contracts::{AdjustmentRevisionCommitReceiptProjectionKindEnum, Nullable};

use super::{
    NativeRevisionSpool, RevisionSpoolLeaseCodec, existing_immutable_name, list_names,
    read_optional_file, read_private_file, spool_error, sync_directory,
};
use crate::permanent_archive_gap::{
    PermanentArchiveGapAddress, PermanentArchiveGapCodec, PermanentArchiveGapInput,
    PermanentArchiveGapKind, PermanentArchiveGapStorage, bounded_gap_bytes,
};
use crate::private_revision_archive::{
    AdmittedPrivateRevisionProjection, PRIVATE_REVISION_STAGE_RECEIPT_MAXIMUM_BYTES,
    PrivateRevisionArchiveStorage, PrivateRevisionExclusiveWrite,
    PrivateRevisionProjectionDependency,
};
use crate::private_shadow_archive::{
    PrivateShadowArchiveMetadataKind, PrivateShadowArchiveStorage, PrivateShadowExclusiveWrite,
};
use crate::rain_control_state_archive::{
    RainControlArchiveMetadataKind, RainControlStateWireCodec, decode_canonical_rain_stage_receipt,
};
use crate::revision_archive_canonical::{is_payload_name, lower_hex};
use crate::revision_online_spool::{
    RevisionOnlinePayloadAddress, RevisionOnlinePayloadKind, RevisionOnlineSpoolError,
    RevisionOnlineSpoolStorage,
};

const REVISION_STAGES: &str = "revision-stage-receipts";
const REVISION_COMMITS: &str = "revision-commit-receipts";
const REVISION_SUCCESSORS: &str = "revision-frontier-successors";
const REVISION_PUBLICATIONS: &str = "revision-publish-receipts";
const REVISION_GAPS: &str = "revision-gaps";
const SHADOW_GAPS: &str = "shadow-revision-gaps";
const METADATA_BYTES: usize = 4_096;
const MAXIMUM_GAPS: usize = 4_096;
type Outcome<T> = Result<T, RevisionOnlineSpoolError>;

/// Exact source parser extensions needed by native archive reads.
pub trait NativeRevisionSpoolArtifactCodec:
    RainControlStateWireCodec + PermanentArchiveGapCodec
{
    /// Parse the canonical monthly state and return its ordinal artifact digest.
    fn admitted_rain_control_ordinal_sha256(&self, bytes: &[u8]) -> Option<String>;
}

impl<Codec: RevisionSpoolLeaseCodec> NativeRevisionSpool<Codec> {
    /// Read one exact optional fixed-directory record by its validated digest.
    fn read_digest(
        &self,
        directory: &str,
        digest: &str,
        maximum: usize,
    ) -> Outcome<Option<Vec<u8>>> {
        let name = Self::metadata_name(digest).map_err(spool_error)?;
        read_optional_file(&self.root.join(directory).join(name), maximum).map_err(spool_error)
    }

    /// Publish one address without changing any already retained document.
    fn write_metadata(
        &self,
        directory: &str,
        digest: &str,
        bytes: &[u8],
        maximum: usize,
    ) -> Outcome<PrivateRevisionExclusiveWrite> {
        // enforce the exact per-record native operation bound
        if bytes.len() > maximum {
            return Err(RevisionOnlineSpoolError::Input);
        }
        let name = Self::metadata_name(digest).map_err(spool_error)?;
        match self.publish_once(directory, &name, bytes) {
            Ok(()) => Ok(PrivateRevisionExclusiveWrite::Written),
            Err(error) if existing_immutable_name(&error) => {
                Ok(PrivateRevisionExclusiveWrite::AlreadyExists)
            }
            Err(_) => Err(RevisionOnlineSpoolError::Storage),
        }
    }

    /// Preserve the source's immutable write-or-verify semantics for an exact address.
    fn write_identical(
        &self,
        directory: &str,
        name: &str,
        bytes: &[u8],
        maximum: usize,
    ) -> Outcome<()> {
        // retain the per-file bound before opening a new temporary
        if bytes.len() > maximum {
            return Err(RevisionOnlineSpoolError::Input);
        }
        match self.publish_once(directory, name, bytes) {
            Ok(()) => Ok(()),
            Err(error) if existing_immutable_name(&error) => {
                let retained = read_private_file(&self.root.join(directory).join(name), maximum)
                    .map_err(spool_error)?;
                // preserve byte-identical retry without replacing its original member
                if retained != bytes {
                    return Err(RevisionOnlineSpoolError::IdentityConflict);
                }
                Ok(())
            }
            Err(_) => Err(RevisionOnlineSpoolError::Storage),
        }
    }

    /// Enumerate a bounded complete permanent-gap set in its retained directory order.
    fn gap_names(&self, directory: &str) -> Outcome<Vec<String>> {
        let names =
            list_names(&self.root.join(directory), Some(MAXIMUM_GAPS + 1)).map_err(spool_error)?;
        // require the complete census before scanning any gap bytes
        if names.len() > MAXIMUM_GAPS || !names.iter().map(String::as_str).all(is_payload_name) {
            return Err(RevisionOnlineSpoolError::Census);
        }
        Ok(names)
    }
}

impl<Codec: RevisionSpoolLeaseCodec + NativeRevisionSpoolArtifactCodec>
    PrivateRevisionArchiveStorage for NativeRevisionSpool<Codec>
{
    /// Verify the fixed owner-private spool used by the retained store.
    fn ensure_revision_archive(&mut self) -> Outcome<()> {
        self.ensure_revision_directories().map_err(spool_error)
    }

    /// Bind a controlled rain projection to actual persisted state and its canonical receipt.
    fn verify_projection_dependencies(
        &mut self,
        projection: &AdmittedPrivateRevisionProjection,
    ) -> Outcome<()> {
        let PrivateRevisionProjectionDependency::RainControlState {
            ordinal_artifact_sha256,
            state_sha256,
            state_stage_receipt_sha256,
        } = &projection.dependency
        else {
            return Ok(());
        };
        let state = self
            .read_digest(
                RevisionOnlinePayloadKind::RainControlState.directory(),
                state_sha256,
                RevisionOnlinePayloadKind::RainControlState.maximum_bytes(),
            )?
            .ok_or(RevisionOnlineSpoolError::Storage)?;
        let ordinal = self
            .codec
            .admitted_rain_control_ordinal_sha256(&state)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        let stage = self
            .read_digest(
                RainControlArchiveMetadataKind::StageReceipt.directory(),
                state_sha256,
                METADATA_BYTES,
            )?
            .ok_or(RevisionOnlineSpoolError::Storage)?;
        let stage = decode_canonical_rain_stage_receipt(&self.codec, &stage, state_sha256)?;
        // require the actual state hash, ordinal artifact, and first durable stage identity
        if lower_hex(self.codec.sha256(&state)) != *state_sha256
            || ordinal != *ordinal_artifact_sha256
            || stage.stageReceiptSha256 != *state_stage_receipt_sha256
        {
            return Err(RevisionOnlineSpoolError::IdentityConflict);
        }
        Ok(())
    }

    /// Do not restore a permanently abandoned state under the shared value write lease.
    fn verify_projection_write_postcondition(
        &mut self,
        projection: &AdmittedPrivateRevisionProjection,
    ) -> Outcome<()> {
        self.require_lease().map_err(spool_error)?;
        // check only the controlled-rain state path used by this projection type
        if let PrivateRevisionProjectionDependency::RainControlState { state_sha256, .. } =
            &projection.dependency
            && self
                .read_digest(
                    RainControlArchiveMetadataKind::GapReceipt.directory(),
                    state_sha256,
                    METADATA_BYTES,
                )?
                .is_some()
        {
            return Err(RevisionOnlineSpoolError::IdentityConflict);
        }
        Ok(())
    }

    /// Publish the exact projection while capacity admission still holds the spool lease.
    fn write_projection_exclusive(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> Outcome<()> {
        let (kind, name) = address.components();
        // keep the write restricted to the admitted projection namespace and bound
        if kind != RevisionOnlinePayloadKind::Projection || bytes.len() > kind.maximum_bytes() {
            return Err(RevisionOnlineSpoolError::Input);
        }
        self.write_once(kind.directory(), name, bytes)
            .map_err(|_| RevisionOnlineSpoolError::Storage)
    }

    /// Read one exact optional retained projection stage receipt.
    fn read_stage_receipt(&mut self, digest: &str, maximum: usize) -> Outcome<Option<Vec<u8>>> {
        self.read_digest(REVISION_STAGES, digest, maximum)
    }

    /// Commit the stage receipt independently of value payload concurrency.
    fn write_stage_receipt_exclusive(
        &mut self,
        digest: &str,
        bytes: &[u8],
    ) -> Outcome<PrivateRevisionExclusiveWrite> {
        self.write_metadata(
            REVISION_STAGES,
            digest,
            bytes,
            PRIVATE_REVISION_STAGE_RECEIPT_MAXIMUM_BYTES,
        )
    }

    /// Bind every retained permanent classification before selecting the requested body.
    fn has_permanent_revision_gap(
        &mut self,
        projection: &str,
        kind: AdjustmentRevisionCommitReceiptProjectionKindEnum,
    ) -> Outcome<bool> {
        let mut found = false;
        let expected_kind = wire_value(
            kind,
            AdjustmentRevisionCommitReceiptProjectionKindEnum::WIRE_VALUES,
        );
        // preserve the complete canonical census even after finding one matching gap
        for name in self.gap_names(REVISION_GAPS)? {
            let path = self.root.join(REVISION_GAPS).join(&name);
            let bytes = read_private_file(&path, METADATA_BYTES).map_err(spool_error)?;
            let gap = self
                .codec
                .accepted_revision_gap(&bytes)
                .ok_or(RevisionOnlineSpoolError::Census)?;
            let projection_identity = match &gap.input.projectionIdentitySha256 {
                Nullable::Null => None,
                Nullable::Value(value) => Some(value.as_str()),
            };
            let decoded = PermanentArchiveGapInput::Revision(&gap.input);
            decoded.validated_kind()?;
            let key = bounded_gap_bytes(self.codec.encode_gap_key(decoded))?;
            // bind the accepted record to the exact source filename preimage
            if name != format!("sha256-{}.json", lower_hex(self.codec.sha256(&key))) {
                return Err(RevisionOnlineSpoolError::IdentityConflict);
            }
            found |= projection_identity == Some(projection)
                && wire_value(
                    gap.input.projectionKind,
                    weather_contracts::AdjustmentRevisionProjectionKind::WIRE_VALUES,
                ) == expected_kind;
        }
        Ok(found)
    }

    /// Publish the single receipt under its authenticated frontier identity.
    fn write_or_verify_commit_receipt(
        &mut self,
        digest: &str,
        bytes: &[u8],
        maximum: usize,
    ) -> Outcome<()> {
        let name = Self::metadata_name(digest).map_err(spool_error)?;
        self.write_identical(REVISION_COMMITS, &name, bytes, maximum)
    }

    /// Publish the direct successor under the predecessor's identity.
    fn write_or_verify_successor(
        &mut self,
        digest: &str,
        bytes: &[u8],
        maximum: usize,
    ) -> Outcome<()> {
        let name = Self::metadata_name(digest).map_err(spool_error)?;
        self.write_identical(REVISION_SUCCESSORS, &name, bytes, maximum)
    }

    /// Retain a grouped chain in its separate exact prefixed namespace.
    fn write_or_verify_batch_commit_group(
        &mut self,
        digest: &str,
        bytes: &[u8],
        maximum: usize,
    ) -> Outcome<()> {
        self.require_lease().map_err(spool_error)?;
        let name = Self::metadata_name(digest).map_err(spool_error)?;
        self.write_identical(REVISION_COMMITS, &format!("batch-{name}"), bytes, maximum)
    }

    /// Read only the first canonical single or grouped publication document.
    fn read_publish_receipt(&mut self, digest: &str, maximum: usize) -> Outcome<Option<Vec<u8>>> {
        self.read_digest(REVISION_PUBLICATIONS, digest, maximum)
    }

    /// Create the next publication root without replacing an already durable receipt.
    fn write_publish_receipt_exclusive(
        &mut self,
        digest: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Outcome<PrivateRevisionExclusiveWrite> {
        self.write_metadata(REVISION_PUBLICATIONS, digest, bytes, maximum_bytes)
    }
}

impl<Codec: RevisionSpoolLeaseCodec + NativeRevisionSpoolArtifactCodec> PrivateShadowArchiveStorage
    for NativeRevisionSpool<Codec>
{
    /// Verify the Node-compatible shadow archive root.
    fn ensure_revision_root(&mut self) -> Outcome<()> {
        self.ensure_revision_directories().map_err(spool_error)
    }

    /// Preserve the exact scheduled gap as a permanent archive outcome.
    fn has_permanent_shadow_gap(&mut self, due_key: &str, digest: &str) -> Outcome<bool> {
        let mut found = false;
        // inspect each accepted gap before finalizing the scheduled admission result
        for name in self.gap_names(SHADOW_GAPS)? {
            let bytes = read_private_file(&self.root.join(SHADOW_GAPS).join(&name), METADATA_BYTES)
                .map_err(spool_error)?;
            let gap = self
                .codec
                .accepted_shadow_gap(&bytes)
                .ok_or(RevisionOnlineSpoolError::Census)?;
            let decoded = PermanentArchiveGapInput::Shadow(&gap.input);
            decoded.validated_kind()?;
            let key = bounded_gap_bytes(self.codec.encode_gap_key(decoded))?;
            // bind value-free metadata to its exact durable file locator
            if name != format!("sha256-{}.json", lower_hex(self.codec.sha256(&key))) {
                return Err(RevisionOnlineSpoolError::IdentityConflict);
            }
            found |= gap.input.dueKey == due_key && gap.input.registrationSha256 == digest;
        }
        Ok(found)
    }

    /// Publish exactly one new value-bearing shadow-stage member.
    fn write_online_exclusive(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> Outcome<()> {
        let (kind, name) = address.components();
        // keep the stage write restricted to admitted shadow storage
        if kind != RevisionOnlinePayloadKind::ShadowStage || bytes.len() > kind.maximum_bytes() {
            return Err(RevisionOnlineSpoolError::Input);
        }
        self.write_once(kind.directory(), name, bytes)
            .map_err(|_| RevisionOnlineSpoolError::Storage)
    }

    /// Inspect the exact admitted immutable shadow member without a write lease.
    fn read_online(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum: usize,
    ) -> Outcome<Option<Vec<u8>>> {
        self.read_existing_payload(address, maximum)
    }

    /// Publish the complete immutable capsule under the same capacity lease.
    fn write_online_immutable(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> Outcome<()> {
        self.require_lease().map_err(spool_error)?;
        let (kind, name) = address.components();
        // permit only the completed shadow capsule on this write path
        if kind != RevisionOnlinePayloadKind::ShadowCapsule {
            return Err(RevisionOnlineSpoolError::Input);
        }
        self.write_identical(kind.directory(), name, bytes, kind.maximum_bytes())
    }

    /// Once the replacement exists, retire only its exact redundant stage.
    fn remove_online_if_present(&mut self, address: &RevisionOnlinePayloadAddress) -> Outcome<()> {
        self.require_lease().map_err(spool_error)?;
        // preserve the existing stage-only cleanup authority
        if address.components().0 != RevisionOnlinePayloadKind::ShadowStage {
            return Err(RevisionOnlineSpoolError::Input);
        }
        match fs::remove_file(self.payload_path(address)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(spool_error(error)),
        }
    }

    /// Sync the fixed stage directory after exact redundant-file retirement.
    fn sync_online_directory(&mut self, kind: RevisionOnlinePayloadKind) -> Outcome<()> {
        self.require_lease().map_err(spool_error)?;
        // sync only the stage directory changed by capsule retirement
        if kind != RevisionOnlinePayloadKind::ShadowStage {
            return Err(RevisionOnlineSpoolError::Input);
        }
        sync_directory(&self.root.join(kind.directory())).map_err(spool_error)
    }

    /// Read one exact shadow publication or cold chain record.
    fn read_metadata(
        &mut self,
        kind: PrivateShadowArchiveMetadataKind,
        digest: &str,
        maximum: usize,
    ) -> Outcome<Option<Vec<u8>>> {
        self.read_digest(kind.directory(), digest, maximum)
    }

    /// Retain cold chain metadata before publishing the shadow capsule.
    fn write_metadata_immutable(
        &mut self,
        kind: PrivateShadowArchiveMetadataKind,
        digest: &str,
        bytes: &[u8],
        maximum: usize,
    ) -> Outcome<()> {
        self.require_lease().map_err(spool_error)?;
        let name = Self::metadata_name(digest).map_err(spool_error)?;
        self.write_identical(kind.directory(), &name, bytes, maximum)
    }

    /// Preserve the independent publication-receipt retry semantics.
    fn write_metadata_exclusive(
        &mut self,
        kind: PrivateShadowArchiveMetadataKind,
        digest: &str,
        bytes: &[u8],
    ) -> Outcome<PrivateShadowExclusiveWrite> {
        // permit independent creation only for the final publication marker
        if kind != PrivateShadowArchiveMetadataKind::PublishReceipt {
            return Err(RevisionOnlineSpoolError::Input);
        }
        self.write_metadata(kind.directory(), digest, bytes, METADATA_BYTES)
            .map(
                // preserve the typed collision state for first-clock reconciliation
                |outcome| match outcome {
                    PrivateRevisionExclusiveWrite::Written => PrivateShadowExclusiveWrite::Created,
                    PrivateRevisionExclusiveWrite::AlreadyExists => {
                        PrivateShadowExclusiveWrite::AlreadyPresent
                    }
                },
            )
    }
}

/// Read the source wire tag for one generated closed enum member.
fn wire_value<T: Copy + PartialEq>(value: T, tags: &[(&'static str, T)]) -> Option<&'static str> {
    tags.iter().find_map(
        // select only the exact canonical generated enum value
        |(wire, tag)| (*tag == value).then_some(*wire),
    )
}

impl<Codec: RevisionSpoolLeaseCodec> PermanentArchiveGapStorage for NativeRevisionSpool<Codec> {
    /// Verify the same private namespace used by all retained revision writers.
    fn ensure_revision_root(&mut self) -> Outcome<()> {
        self.ensure_revision_directories().map_err(spool_error)
    }

    /// Serialize the actual immutable metadata write with the shared Node-compatible lease.
    fn with_permanent_gap_lease<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Outcome<T>,
    ) -> Outcome<T> {
        self.with_lease(operation, RevisionOnlineSpoolError::Storage)
    }

    /// Read one exact typed permanent metadata filename inside the lease.
    fn read_permanent_gap(
        &mut self,
        address: &PermanentArchiveGapAddress,
        maximum_bytes: usize,
    ) -> Outcome<Option<Vec<u8>>> {
        self.require_lease().map_err(spool_error)?;
        let (kind, name) = address.components();
        read_optional_file(&self.root.join(kind.directory()).join(name), maximum_bytes)
            .map_err(spool_error)
    }

    /// Observe at most the needed number of immutable disposition names.
    fn list_permanent_gap_names(
        &mut self,
        kind: PermanentArchiveGapKind,
        maximum: usize,
    ) -> Outcome<Vec<String>> {
        self.require_lease().map_err(spool_error)?;
        list_names(&self.root.join(kind.directory()), Some(maximum)).map_err(spool_error)
    }

    /// Write the actual canonical bytes through exclusive publication and fsync.
    fn write_permanent_gap(
        &mut self,
        address: &PermanentArchiveGapAddress,
        bytes: &[u8],
    ) -> Outcome<()> {
        let (kind, name) = address.components();
        // preserve the deployed small metadata envelope independently of the producer
        if bytes.len() > METADATA_BYTES {
            return Err(RevisionOnlineSpoolError::Input);
        }
        self.write_once(kind.directory(), name, bytes)
            .map_err(|_| RevisionOnlineSpoolError::Storage)
    }
}
