//! Native durable slots and exact hot-file retirement for cold revision custody.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{NativeRevisionSpool, RevisionSpoolLeaseCodec, list_names, require_directory};
use crate::native_revision_spool_file::{create_synced_file, read_optional_file, sync_directory};
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalCodec, is_payload_name, lower_hex,
};
use crate::revision_cold_ack::{
    REVISION_COLD_ACK_MAXIMUM_BYTES, RevisionColdAck, RevisionColdAckError,
    RevisionColdRetirementEntry, RevisionColdRetirementKind,
};
use crate::revision_gap_transfer::RevisionGapSha256;

const ACK_DIRECTORY: &str = "revision-custody-acknowledgements";
const CURRENT_ACK: &str = "current.json";
const PREVIOUS_ACK: &str = "previous.json";
const REVISION_PROJECTION_MAXIMUM_BYTES: usize = 3_500 * 1024;
const RAIN_CONTROL_STATE_MAXIMUM_BYTES: usize = 32 * 1024;
const BATCH_COMMIT_MAXIMUM_BYTES: usize = 256 * 1024;
const SHADOW_CAPSULE_MAXIMUM_BYTES: usize = 2 * 1024 * 1024;
const PUBLISH_RECEIPT_MAXIMUM_BYTES: usize = 32 * 1024;
const METADATA_MAXIMUM_BYTES: usize = 4_096;
const ONLINE_PAYLOAD_MAXIMUM_SLOTS: usize = 2;

const REQUIRED_DIRECTORIES: [&str; 13] = [
    "revision-commit-receipts",
    "revision-frontier-successors",
    "revision-projections",
    "shadow-revision-capsules",
    "rain-control-states",
    "rain-control-state-stage-receipts",
    ACK_DIRECTORY,
    "revision-gaps",
    "revision-publish-receipts",
    "revision-spool-locks",
    "revision-stage-receipts",
    "shadow-revision-gaps",
    "shadow-revision-publish-receipts",
];

const CENSUS_DIRECTORIES: [(&str, bool); 9] = [
    ("revision-commit-receipts", true),
    ("revision-frontier-successors", false),
    ("revision-projections", false),
    ("revision-publish-receipts", false),
    ("revision-stage-receipts", false),
    ("shadow-revision-capsules", false),
    ("shadow-revision-publish-receipts", false),
    ("rain-control-states", false),
    ("rain-control-state-stage-receipts", false),
];

impl<Lease: RevisionSpoolLeaseCodec> NativeRevisionSpool<Lease> {
    /// Require the complete pre-existing custody tree without creating or repairing it.
    pub(crate) fn require_cold_ack_directories(&self) -> Result<(), RevisionColdAckError> {
        require_directory(&self.root).map_err(|_| RevisionColdAckError::Storage)?;
        // preserve the original fixed child order before any acknowledgement read
        for directory in REQUIRED_DIRECTORIES {
            require_directory(&self.root.join(directory))
                .map_err(|_| RevisionColdAckError::Storage)?;
        }
        Ok(())
    }

    /// Admit all nine successful-spool classes before any retirement mutation.
    pub(crate) fn require_cold_ack_census(&self) -> Result<(), RevisionColdAckError> {
        self.require_lease()
            .map_err(|_| RevisionColdAckError::Storage)?;
        // bound every complete directory enumeration to one item beyond its ceiling
        for (directory, permits_batch) in CENSUS_DIRECTORIES {
            let names = list_names(
                &self.root.join(directory),
                Some(ONLINE_PAYLOAD_MAXIMUM_SLOTS + 1),
            )
            .map_err(|_| RevisionColdAckError::Storage)?;
            if names.len() > ONLINE_PAYLOAD_MAXIMUM_SLOTS
                || names
                    .iter()
                    .any(|name| !valid_census_name(name, permits_batch))
            {
                return Err(RevisionColdAckError::Census);
            }
        }
        Ok(())
    }

    /// Read and cross-bind the canonical current and direct predecessor slots.
    pub(crate) fn read_cold_ack_state(
        &self,
        decode: impl Fn(&[u8]) -> Option<RevisionColdAck>,
    ) -> Result<Option<(RevisionColdAck, Vec<u8>)>, RevisionColdAckError> {
        self.require_lease()
            .map_err(|_| RevisionColdAckError::Storage)?;
        let directory = self.root.join(ACK_DIRECTORY);
        let mut names =
            list_names(&directory, Some(3)).map_err(|_| RevisionColdAckError::Storage)?;
        names.sort_unstable();
        // reject hidden or crash-left state rather than treating it as authority
        if names.len() > 2
            || names
                .iter()
                .any(|name| name != CURRENT_ACK && name != PREVIOUS_ACK)
        {
            return Err(RevisionColdAckError::State);
        }
        let current_bytes = read_optional_file(
            &directory.join(CURRENT_ACK),
            REVISION_COLD_ACK_MAXIMUM_BYTES,
        )
        .map_err(|_| RevisionColdAckError::Storage)?;
        let previous_bytes = read_optional_file(
            &directory.join(PREVIOUS_ACK),
            REVISION_COLD_ACK_MAXIMUM_BYTES,
        )
        .map_err(|_| RevisionColdAckError::Storage)?;
        let current = decode_optional_ack(current_bytes.as_deref(), &decode)?;
        let previous = decode_optional_ack(previous_bytes.as_deref(), &decode)?;
        // retain only no history or one exact direct predecessor generation
        let valid_predecessor = match (&current, &previous) {
            (None, None) => true,
            (Some(current), None) => current.previous_acknowledgement_sha256.is_none(),
            (Some(current), Some(previous)) => {
                current.previous_acknowledgement_sha256 == Some(previous.acknowledgement_sha256)
            }
            (None, Some(_)) => false,
        };
        if !valid_predecessor {
            return Err(RevisionColdAckError::State);
        }
        Ok(current.zip(current_bytes))
    }

    /// Durably rotate the admitted current bytes before replacing the current slot.
    pub(crate) fn publish_cold_ack(
        &self,
        previous: Option<&[u8]>,
        current: &[u8],
    ) -> Result<(), RevisionColdAckError> {
        self.require_lease()
            .map_err(|_| RevisionColdAckError::Storage)?;
        // refuse impossible slot material before creating the first temporary
        if current.len() > REVISION_COLD_ACK_MAXIMUM_BYTES
            || previous.is_some_and(|bytes| bytes.len() > REVISION_COLD_ACK_MAXIMUM_BYTES)
        {
            return Err(RevisionColdAckError::Encoding);
        }
        let directory = self.root.join(ACK_DIRECTORY);
        // retain exactly the prior current checkpoint when advancing history
        if let Some(previous) = previous {
            atomic_replace(&directory.join(PREVIOUS_ACK), previous)?;
        }
        atomic_replace(&directory.join(CURRENT_ACK), current)
    }

    /// Verify every retained member before unlinking and synchronizing exact directories.
    pub(crate) fn retire_cold_ack_entries(
        &self,
        entries: &[RevisionColdRetirementEntry],
        codec: &impl RevisionArchiveCanonicalCodec,
    ) -> Result<(), RevisionColdAckError> {
        self.require_lease()
            .map_err(|_| RevisionColdAckError::Storage)?;
        let files = entries
            .iter()
            .copied()
            .map(|entry| retirement_file(&self.root, entry))
            .collect::<Vec<_>>();
        // verify the complete checkpoint-derived set before deleting its first member
        for file in &files {
            let retained = read_optional_file(&file.path, file.maximum_bytes)
                .map_err(|_| RevisionColdAckError::Storage)?;
            if retained.is_some_and(|bytes| {
                RevisionGapSha256::admitted(codec.sha256(&bytes)) != file.expected_sha256
            }) {
                return Err(RevisionColdAckError::Retirement);
            }
        }
        let mut directories = Vec::new();
        // permit only absence caused by a crash after the acknowledgement became durable
        for file in &files {
            match fs::remove_file(&file.path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(RevisionColdAckError::Storage),
            }
            // preserve first-seen directory order for the final durability pass
            if !directories.contains(&file.directory) {
                directories.push(file.directory);
            }
        }
        // make every completed unlink durable before returning acknowledgement success
        for directory in directories {
            sync_directory(&self.root.join(directory))
                .map_err(|_| RevisionColdAckError::Storage)?;
        }
        Ok(())
    }
}

/// Decode one optional slot while rejecting a malformed present file as absence.
fn decode_optional_ack(
    bytes: Option<&[u8]>,
    decode: &impl Fn(&[u8]) -> Option<RevisionColdAck>,
) -> Result<Option<RevisionColdAck>, RevisionColdAckError> {
    match bytes {
        None => Ok(None),
        Some(bytes) => decode(bytes).map(Some).ok_or(RevisionColdAckError::State),
    }
}

/// Retain the sole canonical content-addressed basename grammars per census class.
fn valid_census_name(name: &str, permits_batch: bool) -> bool {
    is_payload_name(name)
        || permits_batch && name.strip_prefix("batch-").is_some_and(is_payload_name)
}

/// Atomically replace one fixed slot through a synced unique temporary.
fn atomic_replace(target: &Path, bytes: &[u8]) -> Result<(), RevisionColdAckError> {
    let directory = target.parent().ok_or(RevisionColdAckError::Storage)?;
    let unique = super::uuid_from_linux_entropy().map_err(|_| RevisionColdAckError::Storage)?;
    let temporary = directory.join(format!(".state-{unique}.tmp"));
    let result = (|| {
        create_synced_file(&temporary, bytes)?;
        fs::rename(&temporary, target)?;
        sync_directory(directory)
    })();
    // remove only the temporary owned by this invocation after a failed transition
    if result.is_err() {
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(RevisionColdAckError::Storage),
        }
    }
    result.map_err(|_| RevisionColdAckError::Storage)
}

/// One fully typed fixed retirement path and its source-owned read ceiling.
struct RetirementFile {
    directory: &'static str,
    expected_sha256: RevisionGapSha256,
    maximum_bytes: usize,
    path: PathBuf,
}

/// Map one closed retirement kind to its only eligible hot-file address.
fn retirement_file(root: &Path, entry: RevisionColdRetirementEntry) -> RetirementFile {
    let (directory, maximum_bytes, prefix) = match entry.kind {
        RevisionColdRetirementKind::RainControlState => (
            "rain-control-states",
            RAIN_CONTROL_STATE_MAXIMUM_BYTES,
            "sha256-",
        ),
        RevisionColdRetirementKind::RainControlStateStageReceipt => (
            "rain-control-state-stage-receipts",
            METADATA_MAXIMUM_BYTES,
            "sha256-",
        ),
        RevisionColdRetirementKind::RevisionCommitGroup => (
            "revision-commit-receipts",
            BATCH_COMMIT_MAXIMUM_BYTES,
            "batch-sha256-",
        ),
        RevisionColdRetirementKind::RevisionCommitReceipt => (
            "revision-commit-receipts",
            METADATA_MAXIMUM_BYTES,
            "sha256-",
        ),
        RevisionColdRetirementKind::RevisionFrontierSuccessor => (
            "revision-frontier-successors",
            METADATA_MAXIMUM_BYTES,
            "sha256-",
        ),
        RevisionColdRetirementKind::RevisionProjection => (
            "revision-projections",
            REVISION_PROJECTION_MAXIMUM_BYTES,
            "sha256-",
        ),
        RevisionColdRetirementKind::RevisionPublishReceipt => (
            "revision-publish-receipts",
            PUBLISH_RECEIPT_MAXIMUM_BYTES,
            "sha256-",
        ),
        RevisionColdRetirementKind::RevisionStageReceipt => {
            ("revision-stage-receipts", METADATA_MAXIMUM_BYTES, "sha256-")
        }
        RevisionColdRetirementKind::RevisionTerminalGap => {
            ("revision-gaps", METADATA_MAXIMUM_BYTES, "sha256-")
        }
        RevisionColdRetirementKind::ShadowRevisionCapsule => (
            "shadow-revision-capsules",
            SHADOW_CAPSULE_MAXIMUM_BYTES,
            "sha256-",
        ),
        RevisionColdRetirementKind::ShadowRevisionPublishReceipt => (
            "shadow-revision-publish-receipts",
            METADATA_MAXIMUM_BYTES,
            "sha256-",
        ),
        RevisionColdRetirementKind::ShadowRevisionTerminalGap => {
            ("shadow-revision-gaps", METADATA_MAXIMUM_BYTES, "sha256-")
        }
    };
    let digest = lower_hex(*entry.identity_sha256.as_bytes());
    RetirementFile {
        directory,
        expected_sha256: entry.file_sha256,
        maximum_bytes,
        path: root.join(directory).join(format!("{prefix}{digest}.json")),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, DirBuilder};
    use std::os::unix::fs::DirBuilderExt;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::native_revision_spool_file::{create_synced_file, read_optional_file};
    use crate::revision_archive_canonical::RevisionArchiveCanonicalCodec;
    use crate::revision_cold_ack::{RevisionColdAckIdentity, RevisionColdAckRequest};
    use crate::revision_cold_page::{RevisionColdCursor, RevisionColdPageRequest};

    struct FixtureCodec;

    impl RevisionSpoolLeaseCodec for FixtureCodec {
        /// Encode one complete fixture lease without a separate JSON owner.
        fn encode_canonical_lease(
            &self,
            expires_at_milliseconds: i64,
            token: &str,
        ) -> Option<Vec<u8>> {
            Some(format!("{expires_at_milliseconds}|{token}").into_bytes())
        }

        /// Decode the retained fixture expiry prefix.
        fn validated_lease_expiry(&self, canonical_document: &[u8]) -> Option<i64> {
            std::str::from_utf8(canonical_document)
                .ok()?
                .split_once('|')?
                .0
                .parse()
                .ok()
        }
    }

    impl RevisionArchiveCanonicalCodec for FixtureCodec {
        /// Timestamps are not part of these storage-effect fixtures.
        fn canonical_utc_milliseconds(&self, _: &str) -> Option<i64> {
            None
        }

        /// Bind exact fixture bytes to their bounded byte length.
        fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
            [u8::try_from(bytes.len()).expect("bounded fixture bytes"); 32]
        }
    }

    struct FixtureRoot(PathBuf);

    impl FixtureRoot {
        /// Create one canonical private root and every pre-existing custody child.
        fn new() -> Self {
            let base = fs::canonicalize(std::env::temp_dir()).expect("temporary root");
            let root = base.join(format!(
                "weather-cold-ack-{}-{}",
                std::process::id(),
                super::super::uuid_from_linux_entropy().expect("fixture identity")
            ));
            private_directory(&root);
            // create only test-owned fixed children before binding the native spool
            for directory in REQUIRED_DIRECTORIES {
                private_directory(&root.join(directory));
            }
            Self(root)
        }

        /// Borrow the disposable canonical root.
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for FixtureRoot {
        /// Remove only the disposable fixture tree.
        fn drop(&mut self) {
            let _cleanup = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    // rotate the current checkpoint and admit only its exact direct predecessor
    fn rotates_and_reads_exact_checkpoint_pair() {
        let root = FixtureRoot::new();
        let mut spool = NativeRevisionSpool::new(root.path().to_path_buf(), FixtureCodec)
            .expect("native spool");
        spool
            .require_cold_ack_directories()
            .expect("fixed custody tree");
        spool
            .with_lease(
                // exercise census, empty state, and both durable checkpoint generations
                |spool| {
                    spool.require_cold_ack_census()?;
                    assert_eq!(spool.read_cold_ack_state(decode_ack)?, None);
                    spool.publish_cold_ack(None, b"ack-1")?;
                    let (first, first_bytes) = spool
                        .read_cold_ack_state(decode_ack)?
                        .expect("first checkpoint");
                    assert_eq!(first.acknowledgement_sha256, hash(1));
                    spool.publish_cold_ack(Some(&first_bytes), b"ack-2")?;
                    let (second, second_bytes) = spool
                        .read_cold_ack_state(decode_ack)?
                        .expect("second checkpoint");
                    assert_eq!(second.previous_acknowledgement_sha256, Some(hash(1)));
                    assert_eq!(second_bytes, b"ack-2");
                    Ok(())
                },
                RevisionColdAckError::Storage,
            )
            .expect("leased checkpoint effects");
    }

    #[test]
    // verify the complete set before unlink and permit a durable crash-retry absence
    fn verifies_all_members_before_retirement() {
        let root = FixtureRoot::new();
        let mut spool = NativeRevisionSpool::new(root.path().to_path_buf(), FixtureCodec)
            .expect("native spool");
        spool
            .require_cold_ack_directories()
            .expect("fixed custody tree");
        let projection = retirement_entry(RevisionColdRetirementKind::RevisionProjection, 7, 3);
        let receipt = retirement_entry(RevisionColdRetirementKind::RevisionCommitReceipt, 8, 4);
        let projection_path = retirement_file(root.path(), projection).path;
        let receipt_path = retirement_file(root.path(), receipt).path;
        create_synced_file(&projection_path, b"one").expect("projection fixture");
        create_synced_file(&receipt_path, b"wrong").expect("receipt fixture");
        spool
            .with_lease(
                // prove an invalid later member preserves every earlier file
                |spool| {
                    assert_eq!(
                        spool.retire_cold_ack_entries(&[projection, receipt], &FixtureCodec),
                        Err(RevisionColdAckError::Retirement)
                    );
                    assert!(projection_path.exists());
                    assert!(receipt_path.exists());
                    fs::remove_file(&receipt_path).map_err(|_| RevisionColdAckError::Storage)?;
                    create_synced_file(&receipt_path, b"four")
                        .map_err(|_| RevisionColdAckError::Storage)?;
                    spool.retire_cold_ack_entries(&[projection, receipt], &FixtureCodec)?;
                    // repeat the same durable set after both unlinks to model crash recovery
                    spool.retire_cold_ack_entries(&[projection, receipt], &FixtureCodec)
                },
                RevisionColdAckError::Storage,
            )
            .expect("leased retirement effects");
        assert_eq!(
            read_optional_file(&projection_path, 64).expect("projection absence"),
            None
        );
        assert_eq!(
            read_optional_file(&receipt_path, 64).expect("receipt absence"),
            None
        );
    }

    /// Create one exact private fixture directory.
    fn private_directory(path: &Path) {
        let mut builder = DirBuilder::new();
        builder.mode(0o700).create(path).expect("private directory");
    }

    /// Return one fixed test identity.
    fn hash(tag: u8) -> RevisionGapSha256 {
        RevisionGapSha256::admitted([tag; 32])
    }

    /// Construct one complete checkpoint with an explicit predecessor identity.
    fn ack(tag: u8, previous: Option<RevisionGapSha256>) -> RevisionColdAck {
        let cursor = RevisionColdCursor {
            ordinal: u64::from(tag),
            frontier: hash(tag),
        };
        let page = RevisionColdPageRequest {
            watermark: cursor,
            start: hash(10),
            after: RevisionColdCursor {
                ordinal: u64::from(tag.saturating_sub(1)),
                frontier: hash(tag.saturating_sub(1)),
            },
            previous: hash(11),
        };
        RevisionColdAck {
            acknowledged_at: format!("fixture-{tag}"),
            request: RevisionColdAckRequest {
                page,
                page_sha256: hash(12),
                identity: RevisionColdAckIdentity::GraphManifest(hash(13)),
                member_root_sha256: hash(14),
                start_member_sha256: None,
            },
            next: cursor,
            previous_acknowledgement_sha256: previous,
            acknowledgement_sha256: hash(tag),
            retirement_entries: Vec::new(),
        }
    }

    /// Decode only the two exact canonical fixture slot bodies.
    fn decode_ack(bytes: &[u8]) -> Option<RevisionColdAck> {
        match bytes {
            b"ack-1" => Some(ack(1, None)),
            b"ack-2" => Some(ack(2, Some(hash(1)))),
            _ => None,
        }
    }

    /// Build one typed fixed-address retirement fixture.
    fn retirement_entry(
        kind: RevisionColdRetirementKind,
        identity: u8,
        bytes: u8,
    ) -> RevisionColdRetirementEntry {
        RevisionColdRetirementEntry {
            kind,
            identity_sha256: hash(identity),
            file_sha256: hash(bytes),
        }
    }
}
