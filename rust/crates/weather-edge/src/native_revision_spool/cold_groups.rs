//! Native selection of direct revision-successor groups for cold archive pages.

use super::{
    NativeRevisionSpool, RevisionSpoolLeaseCodec, list_names, read_optional_file,
    read_private_file, require_directory,
};
use crate::revision_archive_canonical::{is_payload_name, lower_hex};
use crate::revision_gap_transfer::RevisionGapSha256;

const COMMITS: &str = "revision-commit-receipts";
const SUCCESSORS: &str = "revision-frontier-successors";
const ONLINE_PAYLOAD_MAXIMUM_SLOTS: usize = 2;
const SINGLE_METADATA_MAXIMUM_BYTES: usize = 4_096;
const BATCH_COMMIT_MAXIMUM_BYTES: usize = 256 * 1_024;

/// One canonical commit receipt decoded by the source-owned JSON adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRevisionColdReceipt {
    pub archive_commit_ordinal: u64,
    pub frontier_sha256: RevisionGapSha256,
    pub predecessor_frontier_sha256: RevisionGapSha256,
    pub projection_identity_sha256: RevisionGapSha256,
    pub receipt_sha256: RevisionGapSha256,
    pub canonical_bytes: Vec<u8>,
}

/// One canonical successor decoded by the source-owned JSON adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRevisionColdSuccessor {
    pub archive_commit_ordinal: u64,
    pub frontier_sha256: RevisionGapSha256,
    pub predecessor_frontier_sha256: RevisionGapSha256,
    pub receipt_sha256: RevisionGapSha256,
    pub canonical_bytes: Vec<u8>,
}

/// One complete v2 grouped commit body decoded from its canonical file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRevisionColdBatchGroup {
    pub projection_identity_sha256: RevisionGapSha256,
    pub receipts: Vec<NativeRevisionColdReceipt>,
    pub successors: Vec<NativeRevisionColdSuccessor>,
}

/// The exact single or grouped successor selected for one verified cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRevisionColdGroup {
    pub batch: bool,
    pub receipts: Vec<NativeRevisionColdReceipt>,
    pub successors: Vec<NativeRevisionColdSuccessor>,
    pub grouped_bytes: Option<Vec<u8>>,
}

/// Canonical JSON and identity validation retained by the generated contract adapter.
pub trait NativeRevisionColdGroupCodec {
    /// Decode one canonical v1 commit receipt, including every identity check.
    fn admitted_commit_receipt(&self, bytes: &[u8]) -> Option<NativeRevisionColdReceipt>;

    /// Decode one canonical v1 direct successor, including every identity check.
    fn admitted_successor(&self, bytes: &[u8]) -> Option<NativeRevisionColdSuccessor>;

    /// Decode one complete canonical v2 commit group and its nested canonical members.
    fn admitted_batch_commit_group(&self, bytes: &[u8]) -> Option<NativeRevisionColdBatchGroup>;
}

/// Exact local storage and canonical-selection failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRevisionColdGroupError {
    Storage,
    Census,
    Admission,
    Selection,
}

type Outcome<T> = Result<T, NativeRevisionColdGroupError>;

impl<LeaseCodec: RevisionSpoolLeaseCodec> NativeRevisionSpool<LeaseCodec> {
    /// Read the unique single or grouped direct successor for one admitted frontier.
    pub fn read_revision_cold_group(
        &self,
        predecessor_frontier_sha256: RevisionGapSha256,
        codec: &impl NativeRevisionColdGroupCodec,
    ) -> Outcome<NativeRevisionColdGroup> {
        self.require_cold_group_directories()?;
        let successor_path = self
            .root
            .join(SUCCESSORS)
            .join(digest_name(predecessor_frontier_sha256));
        let individual = read_optional_file(&successor_path, SINGLE_METADATA_MAXIMUM_BYTES)
            .map_err(|_| NativeRevisionColdGroupError::Storage)?;
        // preserve the legacy single-receipt representation whenever its address exists
        if let Some(bytes) = individual {
            let successor = codec
                .admitted_successor(&bytes)
                .ok_or(NativeRevisionColdGroupError::Admission)?;
            // bind the canonical body to the predecessor-addressed filename
            if successor.predecessor_frontier_sha256 != predecessor_frontier_sha256 {
                return Err(NativeRevisionColdGroupError::Admission);
            }
            let receipt_bytes = read_private_file(
                &self
                    .root
                    .join(COMMITS)
                    .join(digest_name(successor.frontier_sha256)),
                SINGLE_METADATA_MAXIMUM_BYTES,
            )
            .map_err(|_| NativeRevisionColdGroupError::Storage)?;
            let receipt = codec
                .admitted_commit_receipt(&receipt_bytes)
                .ok_or(NativeRevisionColdGroupError::Admission)?;
            // bind the receipt to its frontier address before returning either body
            if !successor_matches_receipt(&successor, &receipt) {
                return Err(NativeRevisionColdGroupError::Admission);
            }
            return Ok(NativeRevisionColdGroup {
                batch: false,
                receipts: vec![receipt],
                successors: vec![successor],
                grouped_bytes: None,
            });
        }

        let groups = self.read_revision_cold_batch_groups(codec)?;
        let mut selected = None;
        // select exactly one group whose first receipt follows the requested cursor
        for (group, bytes) in groups {
            let follows = group.receipts.first().is_some_and(|receipt| {
                receipt.predecessor_frontier_sha256 == predecessor_frontier_sha256
            });
            // reject an ambiguous direct successor rather than trusting directory order
            if follows && selected.replace((group, bytes)).is_some() {
                return Err(NativeRevisionColdGroupError::Selection);
            }
        }
        let (group, bytes) = selected.ok_or(NativeRevisionColdGroupError::Selection)?;
        Ok(NativeRevisionColdGroup {
            batch: true,
            receipts: group.receipts,
            successors: group.successors,
            grouped_bytes: Some(bytes),
        })
    }

    /// Read every canonical grouped receipt in the complete two-slot hot census.
    fn read_revision_cold_batch_groups(
        &self,
        codec: &impl NativeRevisionColdGroupCodec,
    ) -> Outcome<Vec<(NativeRevisionColdBatchGroup, Vec<u8>)>> {
        let names = list_names(
            &self.root.join(COMMITS),
            Some(ONLINE_PAYLOAD_MAXIMUM_SLOTS + 1),
        )
        .map_err(|_| NativeRevisionColdGroupError::Storage)?;
        // admit the complete fixed census before reading any grouped member
        if names.len() > ONLINE_PAYLOAD_MAXIMUM_SLOTS
            || !names.iter().all(|name| valid_commit_name(name))
        {
            return Err(NativeRevisionColdGroupError::Census);
        }
        let mut groups = Vec::new();
        // leave legacy single members untouched while parsing grouped members
        for name in names.iter().filter(|name| name.starts_with("batch-")) {
            let bytes = read_private_file(
                &self.root.join(COMMITS).join(name),
                BATCH_COMMIT_MAXIMUM_BYTES,
            )
            .map_err(|_| NativeRevisionColdGroupError::Storage)?;
            let group = codec
                .admitted_batch_commit_group(&bytes)
                .ok_or(NativeRevisionColdGroupError::Admission)?;
            // bind the complete canonical group to its projection content address
            if name
                != &format!(
                    "batch-sha256-{}.json",
                    lower_hex(*group.projection_identity_sha256.as_bytes())
                )
            {
                return Err(NativeRevisionColdGroupError::Admission);
            }
            // require a complete nonempty chain with parallel receipt/successor members
            if group.receipts.is_empty()
                || group.receipts.len() != group.successors.len()
                || !group.successors.iter().zip(&group.receipts).all(
                    // bind every nested successor coordinate
                    |(successor, receipt)| successor_matches_receipt(successor, receipt),
                )
            {
                return Err(NativeRevisionColdGroupError::Admission);
            }
            groups.push((group, bytes));
        }
        Ok(groups)
    }

    /// Require only the two pre-existing private directories read by this selector.
    fn require_cold_group_directories(&self) -> Outcome<()> {
        require_directory(&self.root.join(COMMITS))
            .and_then(|()| require_directory(&self.root.join(SUCCESSORS)))
            .map_err(|_| NativeRevisionColdGroupError::Storage)
    }
}

/// Bind every direct successor coordinate to its exact database receipt.
fn successor_matches_receipt(
    successor: &NativeRevisionColdSuccessor,
    receipt: &NativeRevisionColdReceipt,
) -> bool {
    successor.frontier_sha256 == receipt.frontier_sha256
        && successor.predecessor_frontier_sha256 == receipt.predecessor_frontier_sha256
        && successor.archive_commit_ordinal == receipt.archive_commit_ordinal
        && successor.receipt_sha256 == receipt.receipt_sha256
}

/// Retain the source's closed single-or-batch commit filename grammar.
fn valid_commit_name(name: &str) -> bool {
    is_payload_name(name) || name.strip_prefix("batch-").is_some_and(is_payload_name)
}

/// Build the source's fixed lowercase frontier address.
fn digest_name(digest: RevisionGapSha256) -> String {
    format!("sha256-{}.json", lower_hex(*digest.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Lease;

    impl RevisionSpoolLeaseCodec for Lease {
        /// The read-only tests never create a lease document.
        fn encode_canonical_lease(&self, _: i64, _: &str) -> Option<Vec<u8>> {
            None
        }

        /// The read-only tests never admit a lease document.
        fn validated_lease_expiry(&self, _: &[u8]) -> Option<i64> {
            None
        }
    }

    struct Codec;

    impl NativeRevisionColdGroupCodec for Codec {
        /// Decode the compact deterministic receipt fixture.
        fn admitted_commit_receipt(&self, bytes: &[u8]) -> Option<NativeRevisionColdReceipt> {
            let parts = fixture_parts(bytes, "receipt")?;
            Some(receipt(parts[0], parts[1], parts[2], bytes))
        }

        /// Decode the compact deterministic successor fixture.
        fn admitted_successor(&self, bytes: &[u8]) -> Option<NativeRevisionColdSuccessor> {
            let parts = fixture_parts(bytes, "successor")?;
            Some(successor(parts[0], parts[1], parts[2], bytes))
        }

        /// Decode the compact deterministic grouped fixture.
        fn admitted_batch_commit_group(
            &self,
            bytes: &[u8],
        ) -> Option<NativeRevisionColdBatchGroup> {
            let parts = fixture_parts(bytes, "batch")?;
            Some(NativeRevisionColdBatchGroup {
                projection_identity_sha256: digest(parts[0]),
                receipts: vec![receipt(parts[0], parts[1], parts[2], b"nested-receipt")],
                successors: vec![successor(parts[0], parts[1], parts[2], b"nested-successor")],
            })
        }
    }

    // prefer the retained single form
    #[test]
    fn reads_predecessor_addressed_single_before_grouped_files() {
        let fixture = Fixture::new();
        fixture.write(
            SUCCESSORS,
            &digest_name(digest(1)),
            record("successor", 7, 2, 1),
        );
        fixture.write(COMMITS, &digest_name(digest(2)), record("receipt", 7, 2, 1));
        fixture.write(
            COMMITS,
            &format!("batch-{}", digest_name(digest(9))),
            record("batch", 9, 4, 5),
        );

        let spool = NativeRevisionSpool::new(fixture.root.clone(), Lease).expect("spool");
        let group = spool
            .read_revision_cold_group(digest(1), &Codec)
            .expect("single group");

        assert!(!group.batch);
        assert_eq!(group.receipts[0].frontier_sha256, digest(2));
        assert_eq!(group.successors[0].predecessor_frontier_sha256, digest(1));
        assert_eq!(group.grouped_bytes, None);
    }

    // select one direct batch
    #[test]
    fn selects_the_unique_grouped_direct_successor() {
        let fixture = Fixture::new();
        fixture.write(
            COMMITS,
            &format!("batch-{}", digest_name(digest(9))),
            record("batch", 9, 4, 5),
        );
        let spool = NativeRevisionSpool::new(fixture.root.clone(), Lease).expect("spool");

        let group = spool
            .read_revision_cold_group(digest(5), &Codec)
            .expect("batch group");

        assert!(group.batch);
        assert_eq!(group.receipts[0].predecessor_frontier_sha256, digest(5));
        assert_eq!(group.grouped_bytes, Some(record("batch", 9, 4, 5)));
    }

    // reject ambiguous batches
    #[test]
    fn rejects_ambiguous_grouped_direct_successors() {
        let fixture = Fixture::new();
        // both accepted files deliberately claim the same predecessor cursor
        for projection in [9, 10] {
            fixture.write(
                COMMITS,
                &format!("batch-{}", digest_name(digest(projection))),
                record("batch", projection, 4, 5),
            );
        }
        let spool = NativeRevisionSpool::new(fixture.root.clone(), Lease).expect("spool");

        assert_eq!(
            spool.read_revision_cold_group(digest(5), &Codec),
            Err(NativeRevisionColdGroupError::Selection)
        );
    }

    // enforce the shared census
    #[test]
    fn rejects_a_receipt_census_beyond_two_slots() {
        let fixture = Fixture::new();
        // three valid ordinary names exceed the source's entire hot-spool census
        for value in [1, 2, 3] {
            fixture.write(COMMITS, &digest_name(digest(value)), b"unused".to_vec());
        }
        let spool = NativeRevisionSpool::new(fixture.root.clone(), Lease).expect("spool");

        assert_eq!(
            spool.read_revision_cold_group(digest(5), &Codec),
            Err(NativeRevisionColdGroupError::Census)
        );
    }

    /// Disposable owner-private fixture cleaned when each test finishes.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        /// Create the exact two private directories used by the reader.
        fn new() -> Self {
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "weather-cold-groups-{}-{serial}",
                std::process::id()
            ));
            private_directory(&root);
            private_directory(&root.join(COMMITS));
            private_directory(&root.join(SUCCESSORS));
            Self { root }
        }

        /// Write one fixed private fixture member.
        fn write(&self, directory: &str, name: &str, bytes: Vec<u8>) {
            let path = self.root.join(directory).join(name);
            fs::write(&path, bytes).expect("write fixture");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .expect("private file mode");
        }
    }

    impl Drop for Fixture {
        /// Remove only this test's uniquely named temporary tree.
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("remove fixture");
        }
    }

    /// Create one canonical owner-private fixture directory.
    fn private_directory(path: &Path) {
        fs::create_dir(path).expect("create fixture directory");
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .expect("private directory mode");
    }

    /// Return one deterministic admitted digest with its marker in the final byte.
    fn digest(marker: u8) -> RevisionGapSha256 {
        let mut bytes = [0_u8; 32];
        bytes[31] = marker;
        RevisionGapSha256::admitted(bytes)
    }

    /// Encode the tiny parser fixture without pretending it is source JSON.
    fn record(kind: &str, identity: u8, frontier: u8, predecessor: u8) -> Vec<u8> {
        format!("{kind}:{identity}:{frontier}:{predecessor}").into_bytes()
    }

    /// Parse the test-only adapter record into its three marker fields.
    fn fixture_parts(bytes: &[u8], kind: &str) -> Option<Vec<u8>> {
        let text = std::str::from_utf8(bytes).ok()?;
        let fields: Vec<_> = text.split(':').collect();
        // require the fixed fixture tag and exact three numeric markers
        if fields.len() != 4 || fields[0] != kind {
            return None;
        }
        fields[1..]
            .iter()
            .map(|field| field.parse::<u8>().ok())
            .collect()
    }

    /// Build one receipt from the test adapter markers.
    fn receipt(
        identity: u8,
        frontier: u8,
        predecessor: u8,
        bytes: &[u8],
    ) -> NativeRevisionColdReceipt {
        NativeRevisionColdReceipt {
            archive_commit_ordinal: u64::from(frontier),
            frontier_sha256: digest(frontier),
            predecessor_frontier_sha256: digest(predecessor),
            projection_identity_sha256: digest(identity),
            receipt_sha256: digest(frontier.wrapping_add(20)),
            canonical_bytes: bytes.to_vec(),
        }
    }

    /// Build one successor from the test adapter markers.
    fn successor(
        _: u8,
        frontier: u8,
        predecessor: u8,
        bytes: &[u8],
    ) -> NativeRevisionColdSuccessor {
        NativeRevisionColdSuccessor {
            archive_commit_ordinal: u64::from(frontier),
            frontier_sha256: digest(frontier),
            predecessor_frontier_sha256: digest(predecessor),
            receipt_sha256: digest(frontier.wrapping_add(20)),
            canonical_bytes: bytes.to_vec(),
        }
    }
}
