//! Native filesystem binding for one typed revision-gap acknowledgement.

use std::fs;
use std::path::PathBuf;

use super::{NativeRevisionGapMember, NativeRevisionSpool, RevisionSpoolLeaseCodec};
use crate::native_revision_spool_file::{create_synced_file, read_optional_file, sync_directory};
use crate::revision_archive_canonical::lower_hex;
use crate::revision_gap_ack::{
    RevisionGapAckStorage, RevisionGapPayloadAcknowledgement, RevisionGapRetirementDirectory,
    RevisionGapRetirementMember, StoredRevisionGapCheckpoint, StoredRevisionGapPendingPage,
};
use crate::revision_gap_transfer::{RevisionGapPayloadPage, RevisionGapSha256};
use crate::revision_online_spool::{
    RevisionOnlinePayloadAddress, RevisionOnlinePayloadKind, RevisionOnlineSpoolError,
    RevisionOnlineSpoolStorage,
};

const GAP_TRANSFER_DIRECTORY: &str = "revision-gap-transfer";
const CURRENT_CHECKPOINT_NAME: &str = "current.json";
const PENDING_PAGE_NAME: &str = "pending.json";

/// Canonical readers and storage hashing kept separate from the pure mutable identity owner.
pub trait NativeRevisionGapAckDecoder {
    /// Decode one complete canonical acknowledgement without normalizing its retained bytes.
    fn admitted_acknowledgement(
        &self,
        canonical_bytes: &[u8],
    ) -> Option<RevisionGapPayloadAcknowledgement>;

    /// Decode one complete canonical pending page and every native member address.
    fn admitted_pending_page(
        &self,
        canonical_bytes: &[u8],
    ) -> Option<RevisionGapPayloadPage<NativeRevisionGapMember>>;

    /// Hash exact retained filesystem bytes through the admitted SHA-256 owner.
    fn storage_sha256(&self, bytes: &[u8]) -> [u8; 32];
}

/// Borrowed native spool adapter used only for one typed acknowledgement operation.
pub struct NativeRevisionGapAckStorage<'a, LeaseCodec, Decoder> {
    spool: &'a mut NativeRevisionSpool<LeaseCodec>,
    decoder: &'a Decoder,
}

impl<'a, LeaseCodec, Decoder> NativeRevisionGapAckStorage<'a, LeaseCodec, Decoder> {
    /// Bind the shared spool and a distinct canonical decoder without performing I/O.
    pub const fn new(spool: &'a mut NativeRevisionSpool<LeaseCodec>, decoder: &'a Decoder) -> Self {
        Self { spool, decoder }
    }
}

impl<LeaseCodec, Decoder> RevisionOnlineSpoolStorage
    for NativeRevisionGapAckStorage<'_, LeaseCodec, Decoder>
where
    LeaseCodec: RevisionSpoolLeaseCodec,
    Decoder: NativeRevisionGapAckDecoder,
{
    /// Reborrow this adapter inside the parent spool's canonical cross-process lease.
    fn with_revision_spool_lease<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, RevisionOnlineSpoolError>,
    ) -> Result<T, RevisionOnlineSpoolError> {
        // preserve the parent's nonrecursive canonical cross-process lease
        if self.spool.leased {
            return Err(RevisionOnlineSpoolError::Storage);
        }
        let lease = super::Lease::acquire(
            &self.spool.root.join(super::LOCK_DIRECTORY),
            &self.spool.codec,
        )
        .map_err(super::spool_error)?;
        self.spool.leased = true;
        let result = operation(self);
        self.spool.leased = false;
        lease.finish().map_err(super::spool_error)?;
        result
    }

    /// Delegate unrelated exact-payload reads to the parent spool contract.
    fn read_existing_payload(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError> {
        self.spool.read_existing_payload(address, maximum_bytes)
    }

    /// Delegate unrelated bounded payload census reads to the parent spool contract.
    fn list_payload_names(
        &mut self,
        kind: RevisionOnlinePayloadKind,
    ) -> Result<Vec<String>, RevisionOnlineSpoolError> {
        self.spool.list_payload_names(kind)
    }

    /// Delegate unrelated exact payload sizing to the parent spool contract.
    fn payload_file_length(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> Result<usize, RevisionOnlineSpoolError> {
        self.spool.payload_file_length(address, maximum_bytes)
    }
}

impl<LeaseCodec, Decoder> RevisionGapAckStorage<NativeRevisionGapMember>
    for NativeRevisionGapAckStorage<'_, LeaseCodec, Decoder>
where
    LeaseCodec: RevisionSpoolLeaseCodec,
    Decoder: NativeRevisionGapAckDecoder,
{
    /// Create or admit only the parent's fixed owner-private custody tree before locking.
    fn ensure_revision_gap_archive(&mut self) -> Result<(), RevisionOnlineSpoolError> {
        self.spool
            .ensure_revision_directories()
            .map_err(super::spool_error)
    }

    /// Read and decode the exact compact current checkpoint or retain implicit genesis.
    fn read_revision_gap_checkpoint(
        &mut self,
        maximum_bytes: usize,
    ) -> Result<StoredRevisionGapCheckpoint, RevisionOnlineSpoolError> {
        self.spool.require_lease().map_err(super::spool_error)?;
        let path = self.transfer_path(CURRENT_CHECKPOINT_NAME);
        let Some(canonical_bytes) =
            read_optional_file(&path, maximum_bytes).map_err(super::spool_error)?
        else {
            return Ok(StoredRevisionGapCheckpoint::Genesis);
        };
        let acknowledgement = self
            .decoder
            .admitted_acknowledgement(&canonical_bytes)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        Ok(StoredRevisionGapCheckpoint::Acknowledged {
            acknowledgement: Box::new(acknowledgement),
            canonical_bytes,
        })
    }

    /// Read and decode the exact frozen pending page while preserving its original bytes.
    fn read_revision_gap_pending_page(
        &mut self,
        maximum_bytes: usize,
    ) -> Result<
        Option<StoredRevisionGapPendingPage<NativeRevisionGapMember>>,
        RevisionOnlineSpoolError,
    > {
        self.spool.require_lease().map_err(super::spool_error)?;
        let path = self.transfer_path(PENDING_PAGE_NAME);
        let Some(canonical_bytes) =
            read_optional_file(&path, maximum_bytes).map_err(super::spool_error)?
        else {
            return Ok(None);
        };
        let page = self
            .decoder
            .admitted_pending_page(&canonical_bytes)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        Ok(Some(StoredRevisionGapPendingPage {
            canonical_bytes,
            page,
        }))
    }

    /// Replace the compact checkpoint through synced temporary, rename, and directory sync.
    fn write_revision_gap_checkpoint_atomic(
        &mut self,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.spool.require_lease().map_err(super::spool_error)?;
        // enforce the source-owned compact checkpoint ceiling before creating a temporary
        if bytes.len() > maximum_bytes {
            return Err(RevisionOnlineSpoolError::Input);
        }
        let directory = self.spool.root.join(GAP_TRANSFER_DIRECTORY);
        let unique = super::uuid_from_linux_entropy().map_err(super::spool_error)?;
        let temporary = directory.join(format!(".state-{unique}.tmp"));
        let target = directory.join(CURRENT_CHECKPOINT_NAME);
        let result = (|| {
            create_synced_file(&temporary, bytes)?;
            fs::rename(&temporary, &target)?;
            sync_directory(&directory)
        })();

        // remove only this invocation's unique temporary after a failed transition
        if result.is_err() {
            match fs::remove_file(&temporary) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(RevisionOnlineSpoolError::Storage),
            }
        }
        result.map_err(super::spool_error)
    }

    /// Verify one optional addressed member against the hash of its actual retained bytes.
    fn verify_revision_gap_retirement_member(
        &mut self,
        member: RevisionGapRetirementMember,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.spool.require_lease().map_err(super::spool_error)?;
        let path = self.retirement_path(member);
        let retained =
            read_optional_file(&path, member.maximum_bytes).map_err(super::spool_error)?;

        // accept only crash-retry absence or the exact frozen member identity
        if retained.is_some_and(|bytes| {
            RevisionGapSha256::admitted(self.decoder.storage_sha256(&bytes))
                != member.expected_sha256
        }) {
            return Err(RevisionOnlineSpoolError::IdentityConflict);
        }
        Ok(())
    }

    /// Remove one already-verified exact member while accepting crash-retry absence.
    fn remove_revision_gap_retirement_member_if_present(
        &mut self,
        member: RevisionGapRetirementMember,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.spool.require_lease().map_err(super::spool_error)?;
        match fs::remove_file(self.retirement_path(member)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(super::spool_error(error)),
        }
    }

    /// Synchronize the exact fixed retirement directory after its removal.
    fn sync_revision_gap_retirement_directory(
        &mut self,
        directory: RevisionGapRetirementDirectory,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.spool.require_lease().map_err(super::spool_error)?;
        sync_directory(&self.spool.root.join(directory.directory())).map_err(super::spool_error)
    }

    /// Remove the frozen page with distinct fresh and crash-retry absence semantics.
    fn remove_revision_gap_pending_page(
        &mut self,
        allow_absent: bool,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.spool.require_lease().map_err(super::spool_error)?;
        match fs::remove_file(self.transfer_path(PENDING_PAGE_NAME)) {
            Ok(()) => Ok(()),
            Err(error) if allow_absent && error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(super::spool_error(error)),
        }
    }

    /// Synchronize the transfer directory after pending-page retirement.
    fn sync_revision_gap_transfer_directory(&mut self) -> Result<(), RevisionOnlineSpoolError> {
        self.spool.require_lease().map_err(super::spool_error)?;
        sync_directory(&self.spool.root.join(GAP_TRANSFER_DIRECTORY)).map_err(super::spool_error)
    }
}

impl<LeaseCodec, Decoder> NativeRevisionGapAckStorage<'_, LeaseCodec, Decoder> {
    /// Build one fixed transfer file path from a source-owned literal basename.
    fn transfer_path(&self, name: &'static str) -> PathBuf {
        self.spool.root.join(GAP_TRANSFER_DIRECTORY).join(name)
    }

    /// Build one fixed retirement path from typed directory and digest components only.
    fn retirement_path(&self, member: RevisionGapRetirementMember) -> PathBuf {
        let digest = lower_hex(*member.slot_sha256.as_bytes());
        self.spool
            .root
            .join(member.directory.directory())
            .join(format!("sha256-{digest}.json"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, DirBuilder};
    use std::os::unix::fs::DirBuilderExt;
    use std::path::{Path, PathBuf};

    use super::{
        CURRENT_CHECKPOINT_NAME, GAP_TRANSFER_DIRECTORY, NativeRevisionGapAckDecoder,
        NativeRevisionGapAckStorage, PENDING_PAGE_NAME,
    };
    use crate::native_revision_spool::{
        NativeRevisionGapMember, NativeRevisionSpool, RevisionSpoolLeaseCodec,
    };
    use crate::native_revision_spool_file::{
        create_synced_file, read_optional_file, read_private_file,
    };
    use crate::revision_gap_ack::{
        RevisionGapAckStorage, RevisionGapPayloadAcknowledgement, RevisionGapRetirementDirectory,
        RevisionGapRetirementMember,
    };
    use crate::revision_gap_transfer::{RevisionGapPayloadPage, RevisionGapSha256};
    use crate::revision_online_spool::{RevisionOnlineSpoolError, RevisionOnlineSpoolStorage};

    struct FixtureLeaseCodec;

    impl RevisionSpoolLeaseCodec for FixtureLeaseCodec {
        /// Encode one deterministic admitted lease document.
        fn encode_canonical_lease(
            &self,
            expires_at_milliseconds: i64,
            token: &str,
        ) -> Option<Vec<u8>> {
            Some(format!("{expires_at_milliseconds}|{token}").into_bytes())
        }

        /// Decode the fixture's complete lease expiry prefix.
        fn validated_lease_expiry(&self, canonical_document: &[u8]) -> Option<i64> {
            std::str::from_utf8(canonical_document)
                .ok()?
                .split_once('|')?
                .0
                .parse()
                .ok()
        }
    }

    struct FixtureDecoder;

    impl NativeRevisionGapAckDecoder for FixtureDecoder {
        /// Reject checkpoint decoding outside this storage-effect proof.
        fn admitted_acknowledgement(
            &self,
            _canonical_bytes: &[u8],
        ) -> Option<RevisionGapPayloadAcknowledgement> {
            unreachable!("fixture does not read a checkpoint")
        }

        /// Reject pending-page decoding outside this storage-effect proof.
        fn admitted_pending_page(
            &self,
            _canonical_bytes: &[u8],
        ) -> Option<RevisionGapPayloadPage<NativeRevisionGapMember>> {
            unreachable!("fixture does not read a pending page")
        }

        /// Hash fixture bytes by their exact retained length.
        fn storage_sha256(&self, bytes: &[u8]) -> [u8; 32] {
            [u8::try_from(bytes.len()).expect("bounded fixture bytes"); 32]
        }
    }

    struct FixtureRoot(PathBuf);

    impl FixtureRoot {
        /// Create one disposable canonical owner-private root.
        fn new() -> Self {
            let unique = super::super::uuid_from_linux_entropy().expect("fixture UUID");
            let root = std::env::temp_dir().join(format!("weather-gap-ack-{unique}"));
            let mut builder = DirBuilder::new();
            builder.mode(0o700).create(&root).expect("fixture root");
            Self(root)
        }

        /// Borrow the canonical fixture root path.
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for FixtureRoot {
        /// Remove every disposable fixture file at test completion.
        fn drop(&mut self) {
            let _cleanup = fs::remove_dir_all(&self.0);
        }
    }

    // verifies atomic checkpoint replacement and exact leased retirement effects
    #[test]
    fn writes_checkpoint_and_retires_exact_native_members() {
        let root = FixtureRoot::new();
        let mut spool = NativeRevisionSpool::new(root.path().to_path_buf(), FixtureLeaseCodec)
            .expect("native spool");
        spool
            .ensure_revision_directories()
            .expect("fixed spool directories");
        let member = RevisionGapRetirementMember {
            directory: RevisionGapRetirementDirectory::RevisionProjections,
            expected_sha256: RevisionGapSha256::admitted([6; 32]),
            maximum_bytes: 64,
            slot_sha256: RevisionGapSha256::admitted([1; 32]),
        };
        let member_path = root.path().join("revision-projections").join(format!(
            "sha256-{}.json",
            crate::revision_archive_canonical::lower_hex(*member.slot_sha256.as_bytes())
        ));
        let pending_path = root
            .path()
            .join(GAP_TRANSFER_DIRECTORY)
            .join(PENDING_PAGE_NAME);
        create_synced_file(&member_path, b"member").expect("retirement member");
        create_synced_file(&pending_path, b"pending").expect("pending page");
        let decoder = FixtureDecoder;
        let mut storage = NativeRevisionGapAckStorage::new(&mut spool, &decoder);
        storage
            .with_revision_spool_lease(
                // perform the exact checkpoint-before-retirement filesystem ordering
                |storage| {
                    storage.write_revision_gap_checkpoint_atomic(b"checkpoint\n", 4_096)?;
                    storage.verify_revision_gap_retirement_member(member)?;
                    storage.remove_revision_gap_retirement_member_if_present(member)?;
                    storage.sync_revision_gap_retirement_directory(member.directory)?;
                    storage.remove_revision_gap_pending_page(false)?;
                    storage.sync_revision_gap_transfer_directory()?;
                    Ok::<(), RevisionOnlineSpoolError>(())
                },
            )
            .expect("native acknowledgement effects");

        assert_eq!(
            read_private_file(
                &root
                    .path()
                    .join(GAP_TRANSFER_DIRECTORY)
                    .join(CURRENT_CHECKPOINT_NAME),
                4_096,
            )
            .expect("checkpoint bytes"),
            b"checkpoint\n"
        );
        assert_eq!(
            read_optional_file(&member_path, 64).expect("member absence"),
            None
        );
        assert_eq!(
            read_optional_file(&pending_path, 64).expect("pending absence"),
            None
        );
    }
}
