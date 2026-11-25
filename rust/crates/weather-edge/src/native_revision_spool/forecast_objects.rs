//! Native immutable object and first-receipt storage for forecast evidence.

use std::io;
use std::path::{Path, PathBuf};

use crate::native_revision_spool_file::{publish_immutable, read_optional_file, require_directory};
use crate::revision_archive_canonical::lower_hex;

/// maximum retained gzip bytes for one forecast evidence object.
pub const FORECAST_OBJECT_MAXIMUM_BYTES: usize = 16 * 1_024;
/// maximum retained bytes for one legacy first-issuance receipt.
pub const FORECAST_RECEIPT_MAXIMUM_BYTES: usize = 16 * 1_024;
/// maximum retained bytes for one compact v2 first-issuance receipt.
pub const FORECAST_RECEIPT_V2_MAXIMUM_BYTES: usize = 2_048;

const OBJECT_DIRECTORY: &str = "objects";
const RECEIPT_DIRECTORY: &str = "receipts";

/// one already-admitted lowercase SHA-256 identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForecastObjectSha256([u8; 32]);

impl ForecastObjectSha256 {
    /// retain one digest admitted by the existing hash owner.
    pub const fn admitted(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// borrow the fixed digest bytes for codec and accounting ports.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// receipt version selecting the source-owned independent byte ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastObjectReceiptVersion {
    V1,
    V2,
}

impl ForecastObjectReceiptVersion {
    /// return the exact retained receipt ceiling for this version.
    pub const fn maximum_bytes(self) -> usize {
        match self {
            Self::V1 => FORECAST_RECEIPT_MAXIMUM_BYTES,
            Self::V2 => FORECAST_RECEIPT_V2_MAXIMUM_BYTES,
        }
    }
}

/// one complete canonical receipt returned by the existing codec owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedForecastObjectReceipt {
    pub bytes: Vec<u8>,
    pub edge_receipt_identity_sha256: ForecastObjectSha256,
    pub object_sha256: ForecastObjectSha256,
    pub version: ForecastObjectReceiptVersion,
}

/// minimal binding recovered from one fully validated retained receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedForecastObjectReceipt {
    pub object_sha256: ForecastObjectSha256,
    pub version: ForecastObjectReceiptVersion,
}

/// exact gzip/hash and versioned receipt codec retained outside native storage.
pub trait ForecastObjectCommitCodec<Prepared> {
    /// bind one prepared canonical object to its admitted SHA-256 identity.
    fn object_sha256(
        &self,
        prepared: &Prepared,
        compressed_object: &[u8],
    ) -> Option<ForecastObjectSha256>;

    /// bind one stable prepared record identity to its admitted receipt address.
    fn edge_receipt_identity_sha256(&self, prepared: &Prepared) -> Option<ForecastObjectSha256>;

    /// encode one complete first-observed receipt using the source canonical codec.
    fn encode_receipt(
        &self,
        prepared: &Prepared,
        first_edge_committed_at: &str,
        object_sha256: ForecastObjectSha256,
        edge_receipt_identity_sha256: ForecastObjectSha256,
    ) -> Option<EncodedForecastObjectReceipt>;

    /// decode and validate one retained receipt for the expected stable identity.
    fn decode_receipt(
        &self,
        expected_identity_sha256: ForecastObjectSha256,
        canonical_bytes: &[u8],
    ) -> Option<DecodedForecastObjectReceipt>;
}

/// allocation facts passed to the authorized statfs and accounting owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastObjectAllocationRequest {
    pub edge_receipt_identity_sha256: ForecastObjectSha256,
    pub object_bytes: usize,
    pub object_exists: bool,
    pub object_sha256: ForecastObjectSha256,
    pub receipt_bytes: usize,
    pub receipt_exists: bool,
}

/// authorized allocation decision before the first filesystem mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastObjectAllocationDecision {
    Admit,
    Refuse,
}

/// one publication observation retained by the accounting owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastObjectPublication {
    pub bytes: usize,
    pub created: bool,
    pub identity_sha256: ForecastObjectSha256,
    pub kind: ForecastObjectPublicationKind,
}

/// closed immutable file kind reported after each publication attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastObjectPublicationKind {
    Object,
    Receipt,
}

/// authorized statfs, capacity, and allocation accounting boundary.
pub trait ForecastObjectAllocationPort {
    type Error;

    /// admit or refuse one prospective object plus receipt mutation.
    fn admit(
        &mut self,
        request: ForecastObjectAllocationRequest,
    ) -> Result<ForecastObjectAllocationDecision, Self::Error>;

    /// observe each created or already-present immutable publication in source order.
    fn observe(&mut self, publication: ForecastObjectPublication);
}

/// one immutable object commit after forecast preparation and gzip compression.
pub struct ForecastObjectCommitInput<'a, Prepared> {
    pub compressed_object: &'a [u8],
    pub first_edge_committed_at: &'a str,
    pub prepared: &'a Prepared,
}

/// ordinary completed or refused outcomes from the immutable storage slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastObjectCommitOutcome {
    Created { object_created: bool },
    Duplicate,
    IdentityCollision,
    Refused(ForecastObjectCommitRefusal),
}

/// refusals that occur before this storage slice mutates the filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastObjectCommitRefusal {
    Allocation,
    ObjectTooLarge,
}

/// integrity and native failures distinct from ordinary commit outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastObjectCommitError {
    Accounting,
    Codec,
    ObjectCollision,
    ObjectPublication,
    ReceiptPublication { orphan_object: bool },
    ReceiptInvalid,
    StoredObjectDiffers,
    Storage,
}

/// concrete handle for the existing forecast evidence object and receipt directories.
pub struct NativeForecastObjectStore {
    root: PathBuf,
}

impl NativeForecastObjectStore {
    /// admit one existing private root and its fixed object and receipt children.
    pub fn new(root: PathBuf) -> io::Result<Self> {
        require_directory(&root)?;
        require_directory(&root.join(OBJECT_DIRECTORY))?;
        require_directory(&root.join(RECEIPT_DIRECTORY))?;
        Ok(Self { root })
    }

    /// borrow the admitted adjustment-evidence root without exposing mutable paths.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// publish one content-addressed gzip object and one stable first receipt.
    pub fn commit<Prepared, Codec, Allocation>(
        &mut self,
        input: ForecastObjectCommitInput<'_, Prepared>,
        codec: &Codec,
        allocation: &mut Allocation,
    ) -> Result<ForecastObjectCommitOutcome, ForecastObjectCommitError>
    where
        Codec: ForecastObjectCommitCodec<Prepared>,
        Allocation: ForecastObjectAllocationPort,
    {
        // preserve the source gzip object ceiling before any filesystem work
        if input.compressed_object.len() > FORECAST_OBJECT_MAXIMUM_BYTES {
            return Ok(ForecastObjectCommitOutcome::Refused(
                ForecastObjectCommitRefusal::ObjectTooLarge,
            ));
        }
        let object_sha256 = codec
            .object_sha256(input.prepared, input.compressed_object)
            .ok_or(ForecastObjectCommitError::Codec)?;
        let edge_receipt_identity_sha256 = codec
            .edge_receipt_identity_sha256(input.prepared)
            .ok_or(ForecastObjectCommitError::Codec)?;
        let object_path = self.object_path(object_sha256);
        let receipt_path = self.receipt_path(edge_receipt_identity_sha256);
        let existing_receipt = read_optional_file(&receipt_path, FORECAST_RECEIPT_MAXIMUM_BYTES)
            .map_err(|_| ForecastObjectCommitError::Storage)?;

        // honor the first stable receipt before encoding or allocating new bytes
        if let Some(existing_receipt) = existing_receipt {
            return self.finish_duplicate::<Prepared, Codec>(
                codec,
                edge_receipt_identity_sha256,
                object_sha256,
                input.compressed_object,
                &existing_receipt,
            );
        }

        let existing_object = read_optional_file(&object_path, FORECAST_OBJECT_MAXIMUM_BYTES)
            .map_err(|_| ForecastObjectCommitError::Storage)?;
        let receipt = codec
            .encode_receipt(
                input.prepared,
                input.first_edge_committed_at,
                object_sha256,
                edge_receipt_identity_sha256,
            )
            .ok_or(ForecastObjectCommitError::Codec)?;
        // bind the canonical receipt output to both exact storage addresses
        if receipt.edge_receipt_identity_sha256 != edge_receipt_identity_sha256
            || receipt.object_sha256 != object_sha256
        {
            return Err(ForecastObjectCommitError::Codec);
        }
        // enforce the independent versioned receipt ceiling before allocation
        if receipt.bytes.len() > receipt.version.maximum_bytes() {
            return Ok(ForecastObjectCommitOutcome::Refused(
                ForecastObjectCommitRefusal::ObjectTooLarge,
            ));
        }
        let allocation_request = ForecastObjectAllocationRequest {
            edge_receipt_identity_sha256,
            object_bytes: input.compressed_object.len(),
            object_exists: existing_object.is_some(),
            object_sha256,
            receipt_bytes: receipt.bytes.len(),
            receipt_exists: false,
        };
        // retain statfs and allocation policy in its authorized owner
        if allocation
            .admit(allocation_request)
            .map_err(|_| ForecastObjectCommitError::Accounting)?
            == ForecastObjectAllocationDecision::Refuse
        {
            return Ok(ForecastObjectCommitOutcome::Refused(
                ForecastObjectCommitRefusal::Allocation,
            ));
        }

        let object_created = self
            .publish_or_verify(
                &object_path,
                input.compressed_object,
                ForecastObjectCommitError::ObjectCollision,
            )
            .map_err(|cause| match cause {
                ForecastObjectCommitError::ObjectCollision => cause,
                _ => ForecastObjectCommitError::ObjectPublication,
            })?;
        allocation.observe(ForecastObjectPublication {
            bytes: input.compressed_object.len(),
            created: object_created,
            identity_sha256: object_sha256,
            kind: ForecastObjectPublicationKind::Object,
        });

        let receipt_created = self
            .publish_receipt(&receipt_path, &receipt.bytes)
            .map_err(
                // distinguish an orphan from a failed duplicate read
                |_| ForecastObjectCommitError::ReceiptPublication {
                    orphan_object: object_created,
                },
            )?;
        allocation.observe(ForecastObjectPublication {
            bytes: receipt.bytes.len(),
            created: receipt_created,
            identity_sha256: edge_receipt_identity_sha256,
            kind: ForecastObjectPublicationKind::Receipt,
        });
        // preserve a concurrently installed first receipt as authoritative
        if !receipt_created {
            let concurrent = read_optional_file(&receipt_path, FORECAST_RECEIPT_MAXIMUM_BYTES)
                .map_err(|_| ForecastObjectCommitError::Storage)?
                .ok_or(ForecastObjectCommitError::ReceiptInvalid)?;
            let binding = codec
                .decode_receipt(edge_receipt_identity_sha256, &concurrent)
                .ok_or(ForecastObjectCommitError::ReceiptInvalid)?;
            if binding.object_sha256 != object_sha256 {
                return Ok(ForecastObjectCommitOutcome::IdentityCollision);
            }
            return Ok(ForecastObjectCommitOutcome::Duplicate);
        }
        Ok(ForecastObjectCommitOutcome::Created { object_created })
    }

    /// finish the receipt-first retry path without encoding or allocation work.
    fn finish_duplicate<Prepared, Codec>(
        &self,
        codec: &Codec,
        edge_receipt_identity_sha256: ForecastObjectSha256,
        object_sha256: ForecastObjectSha256,
        compressed_object: &[u8],
        receipt_bytes: &[u8],
    ) -> Result<ForecastObjectCommitOutcome, ForecastObjectCommitError>
    where
        Codec: ForecastObjectCommitCodec<Prepared>,
    {
        let receipt = codec
            .decode_receipt(edge_receipt_identity_sha256, receipt_bytes)
            .ok_or(ForecastObjectCommitError::ReceiptInvalid)?;
        // block one stable identity already bound to different scoring content
        if receipt.object_sha256 != object_sha256 {
            return Ok(ForecastObjectCommitOutcome::IdentityCollision);
        }
        let existing_object = read_optional_file(
            &self.object_path(object_sha256),
            FORECAST_OBJECT_MAXIMUM_BYTES,
        )
        .map_err(|_| ForecastObjectCommitError::Storage)?
        .ok_or(ForecastObjectCommitError::StoredObjectDiffers)?;
        // require the receipt's immutable object to remain byte-identical
        if existing_object != compressed_object {
            return Err(ForecastObjectCommitError::StoredObjectDiffers);
        }
        Ok(ForecastObjectCommitOutcome::Duplicate)
    }

    /// publish one object or verify an already-retained byte-identical value.
    fn publish_or_verify(
        &self,
        target: &Path,
        bytes: &[u8],
        collision: ForecastObjectCommitError,
    ) -> Result<bool, ForecastObjectCommitError> {
        let temporary = self.temporary_path(target)?;
        match publish_immutable(target, &temporary, bytes) {
            Ok(()) => Ok(true),
            Err(error)
                if error.source.kind() == io::ErrorKind::AlreadyExists
                    && !error.temporary_cleanup_failed =>
            {
                let retained = read_optional_file(target, FORECAST_OBJECT_MAXIMUM_BYTES)
                    .map_err(|_| ForecastObjectCommitError::Storage)?
                    .ok_or(ForecastObjectCommitError::Storage)?;
                if retained == bytes {
                    Ok(false)
                } else {
                    Err(collision)
                }
            }
            Err(_) => Err(ForecastObjectCommitError::Storage),
        }
    }

    /// publish one receipt while leaving any stable concurrent first writer authoritative.
    fn publish_receipt(
        &self,
        target: &Path,
        bytes: &[u8],
    ) -> Result<bool, ForecastObjectCommitError> {
        let temporary = self.temporary_path(target)?;
        match publish_immutable(target, &temporary, bytes) {
            Ok(()) => Ok(true),
            Err(error)
                if error.source.kind() == io::ErrorKind::AlreadyExists
                    && !error.temporary_cleanup_failed =>
            {
                Ok(false)
            }
            Err(_) => Err(ForecastObjectCommitError::Storage),
        }
    }

    /// derive one unique source-shaped temporary beside its final target.
    fn temporary_path(&self, target: &Path) -> Result<PathBuf, ForecastObjectCommitError> {
        let directory = target.parent().ok_or(ForecastObjectCommitError::Storage)?;
        let unique =
            super::uuid_from_linux_entropy().map_err(|_| ForecastObjectCommitError::Storage)?;
        Ok(directory.join(format!(".capture-{unique}.tmp")))
    }

    /// derive the exact gzip object path from its admitted canonical-content digest.
    fn object_path(&self, object_sha256: ForecastObjectSha256) -> PathBuf {
        self.root.join(OBJECT_DIRECTORY).join(format!(
            "sha256-{}.json.gz",
            lower_hex(*object_sha256.as_bytes())
        ))
    }

    /// derive the exact first-receipt path from its stable record identity.
    fn receipt_path(&self, identity_sha256: ForecastObjectSha256) -> PathBuf {
        self.root.join(RECEIPT_DIRECTORY).join(format!(
            "sha256-{}.json",
            lower_hex(*identity_sha256.as_bytes())
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs::{self, DirBuilder, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct Prepared {
        object: u8,
        receipt: u8,
        version: ForecastObjectReceiptVersion,
    }

    #[derive(Default)]
    struct FixtureCodec {
        encode_calls: Cell<usize>,
    }

    impl ForecastObjectCommitCodec<Prepared> for FixtureCodec {
        /// bind fixture gzip bytes to the prepared object digest.
        fn object_sha256(
            &self,
            prepared: &Prepared,
            compressed_object: &[u8],
        ) -> Option<ForecastObjectSha256> {
            compressed_object
                .starts_with(&[0x1f, 0x8b])
                .then(|| digest(prepared.object))
        }

        /// return the fixture's stable receipt identity.
        fn edge_receipt_identity_sha256(
            &self,
            prepared: &Prepared,
        ) -> Option<ForecastObjectSha256> {
            Some(digest(prepared.receipt))
        }

        /// encode one closed fixture receipt and count receipt creation.
        fn encode_receipt(
            &self,
            prepared: &Prepared,
            first_edge_committed_at: &str,
            object_sha256: ForecastObjectSha256,
            edge_receipt_identity_sha256: ForecastObjectSha256,
        ) -> Option<EncodedForecastObjectReceipt> {
            self.encode_calls.set(self.encode_calls.get() + 1);
            let version = match prepared.version {
                ForecastObjectReceiptVersion::V1 => 1,
                ForecastObjectReceiptVersion::V2 => 2,
            };
            let mut bytes = vec![
                version,
                edge_receipt_identity_sha256.as_bytes()[0],
                object_sha256.as_bytes()[0],
            ];
            bytes.extend_from_slice(first_edge_committed_at.as_bytes());
            Some(EncodedForecastObjectReceipt {
                bytes,
                edge_receipt_identity_sha256,
                object_sha256,
                version: prepared.version,
            })
        }

        /// decode the complete fixture receipt for one expected stable identity.
        fn decode_receipt(
            &self,
            expected_identity_sha256: ForecastObjectSha256,
            canonical_bytes: &[u8],
        ) -> Option<DecodedForecastObjectReceipt> {
            let version = match canonical_bytes.first().copied()? {
                1 => ForecastObjectReceiptVersion::V1,
                2 => ForecastObjectReceiptVersion::V2,
                _ => return None,
            };
            // bind the retained receipt to its addressed stable identity
            if canonical_bytes.get(1).copied()? != expected_identity_sha256.as_bytes()[0] {
                return None;
            }
            Some(DecodedForecastObjectReceipt {
                object_sha256: digest(canonical_bytes.get(2).copied()?),
                version,
            })
        }
    }

    struct FixtureAllocation {
        admissions: Vec<ForecastObjectAllocationRequest>,
        decision: ForecastObjectAllocationDecision,
        observations: Vec<ForecastObjectPublication>,
    }

    impl FixtureAllocation {
        /// create one admitted allocation fixture.
        fn admitted() -> Self {
            Self {
                admissions: Vec::new(),
                decision: ForecastObjectAllocationDecision::Admit,
                observations: Vec::new(),
            }
        }

        /// create one ordinary allocation-refusal fixture.
        fn refused() -> Self {
            Self {
                decision: ForecastObjectAllocationDecision::Refuse,
                ..Self::admitted()
            }
        }
    }

    impl ForecastObjectAllocationPort for FixtureAllocation {
        type Error = ();

        /// retain and return the selected allocation decision.
        fn admit(
            &mut self,
            request: ForecastObjectAllocationRequest,
        ) -> Result<ForecastObjectAllocationDecision, Self::Error> {
            self.admissions.push(request);
            Ok(self.decision)
        }

        /// retain each publication observation in source order.
        fn observe(&mut self, publication: ForecastObjectPublication) {
            self.observations.push(publication);
        }
    }

    struct RacingAllocation {
        receipt_bytes: Vec<u8>,
        receipt_path: PathBuf,
        observations: Vec<ForecastObjectPublication>,
    }

    impl ForecastObjectAllocationPort for RacingAllocation {
        type Error = ();

        /// install one concurrent first receipt after storage's receipt-first read.
        fn admit(
            &mut self,
            _request: ForecastObjectAllocationRequest,
        ) -> Result<ForecastObjectAllocationDecision, Self::Error> {
            write_private(&self.receipt_path, &self.receipt_bytes);
            Ok(ForecastObjectAllocationDecision::Admit)
        }

        /// retain object and raced-receipt publication observations.
        fn observe(&mut self, publication: ForecastObjectPublication) {
            self.observations.push(publication);
        }
    }

    struct DisposableRoot(PathBuf);

    impl DisposableRoot {
        /// create one isolated owner-private forecast object tree.
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("fixture clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "weather-forecast-objects-{}-{nonce}",
                std::process::id()
            ));
            create_private_directory(&root);
            create_private_directory(&root.join(OBJECT_DIRECTORY));
            create_private_directory(&root.join(RECEIPT_DIRECTORY));
            Self(root)
        }
    }

    impl Drop for DisposableRoot {
        /// remove only the disposable fixture tree.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// create one exact owner-private directory.
    fn create_private_directory(path: &Path) {
        let mut builder = DirBuilder::new();
        builder.mode(0o700).create(path).expect("fixture directory");
    }

    /// create one exact owner-private immutable fixture file.
    fn write_private(path: &Path, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .expect("fixture file");
        file.write_all(bytes).expect("fixture bytes");
        file.sync_all().expect("fixture sync");
    }

    /// construct one repeated-byte digest fixture.
    fn digest(byte: u8) -> ForecastObjectSha256 {
        ForecastObjectSha256::admitted([byte; 32])
    }

    /// construct one source-shaped fixture input.
    fn input<'a>(
        prepared: &'a Prepared,
        compressed_object: &'a [u8],
        instant: &'a str,
    ) -> ForecastObjectCommitInput<'a, Prepared> {
        ForecastObjectCommitInput {
            compressed_object,
            first_edge_committed_at: instant,
            prepared,
        }
    }

    // verify first creation and receipt-first retry
    #[test]
    fn creates_exact_object_and_receipt_then_short_circuits_duplicate() {
        let root = DisposableRoot::new();
        let mut store = NativeForecastObjectStore::new(root.0.clone()).expect("store");
        let codec = FixtureCodec::default();
        let mut allocation = FixtureAllocation::admitted();
        let prepared = Prepared {
            object: 1,
            receipt: 2,
            version: ForecastObjectReceiptVersion::V1,
        };
        let compressed = [0x1f, 0x8b, 3, 4];

        assert_eq!(
            store.commit(
                input(&prepared, &compressed, "2026-11-23T12:00:00.000Z"),
                &codec,
                &mut allocation,
            ),
            Ok(ForecastObjectCommitOutcome::Created {
                object_created: true
            })
        );
        assert_eq!(
            fs::read(store.object_path(digest(1))).expect("object bytes"),
            compressed
        );
        let receipt_bytes = fs::read(store.receipt_path(digest(2))).expect("receipt bytes");
        assert_eq!(receipt_bytes[0..3], [1, 2, 1]);
        assert_eq!(codec.encode_calls.get(), 1);
        assert_eq!(allocation.admissions.len(), 1);
        assert_eq!(allocation.observations.len(), 2);

        assert_eq!(
            store.commit(
                input(&prepared, &compressed, "2026-11-23T13:00:00.000Z"),
                &codec,
                &mut allocation,
            ),
            Ok(ForecastObjectCommitOutcome::Duplicate)
        );
        assert_eq!(codec.encode_calls.get(), 1);
        assert_eq!(allocation.admissions.len(), 1);
        assert_eq!(allocation.observations.len(), 2);
        assert!(
            fs::read_dir(root.0.join(OBJECT_DIRECTORY))
                .expect("object directory")
                .all(|entry| !entry
                    .expect("object entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".capture-"))
        );
    }

    // verify stable-identity collision before object publication
    #[test]
    fn distinguishes_identity_collision_without_writing_the_second_object() {
        let root = DisposableRoot::new();
        let mut store = NativeForecastObjectStore::new(root.0.clone()).expect("store");
        let codec = FixtureCodec::default();
        let mut allocation = FixtureAllocation::admitted();
        let first = Prepared {
            object: 1,
            receipt: 2,
            version: ForecastObjectReceiptVersion::V1,
        };
        let collision = Prepared { object: 3, ..first };
        store
            .commit(
                input(&first, &[0x1f, 0x8b, 1], "2026-11-23T12:00:00.000Z"),
                &codec,
                &mut allocation,
            )
            .expect("first commit");

        assert_eq!(
            store.commit(
                input(&collision, &[0x1f, 0x8b, 3], "2026-11-23T13:00:00.000Z",),
                &codec,
                &mut allocation,
            ),
            Ok(ForecastObjectCommitOutcome::IdentityCollision)
        );
        assert!(!store.object_path(digest(3)).exists());
        assert_eq!(codec.encode_calls.get(), 1);
    }

    // verify duplicate object bytes remain exact
    #[test]
    fn rejects_changed_bytes_behind_an_existing_receipt() {
        let root = DisposableRoot::new();
        let mut store = NativeForecastObjectStore::new(root.0.clone()).expect("store");
        let codec = FixtureCodec::default();
        let mut allocation = FixtureAllocation::admitted();
        let prepared = Prepared {
            object: 1,
            receipt: 2,
            version: ForecastObjectReceiptVersion::V1,
        };
        store
            .commit(
                input(&prepared, &[0x1f, 0x8b, 1], "2026-11-23T12:00:00.000Z"),
                &codec,
                &mut allocation,
            )
            .expect("first commit");

        assert_eq!(
            store.commit(
                input(&prepared, &[0x1f, 0x8b, 9], "2026-11-23T13:00:00.000Z"),
                &codec,
                &mut allocation,
            ),
            Err(ForecastObjectCommitError::StoredObjectDiffers)
        );
    }

    // verify allocation and size refusal stay mutation-free
    #[test]
    fn keeps_allocation_and_size_refusals_distinct_from_collisions() {
        let root = DisposableRoot::new();
        let mut store = NativeForecastObjectStore::new(root.0.clone()).expect("store");
        let codec = FixtureCodec::default();
        let prepared = Prepared {
            object: 1,
            receipt: 2,
            version: ForecastObjectReceiptVersion::V2,
        };
        let mut refused = FixtureAllocation::refused();
        assert_eq!(
            store.commit(
                input(&prepared, &[0x1f, 0x8b, 1], "2026-11-23T12:00:00.000Z"),
                &codec,
                &mut refused,
            ),
            Ok(ForecastObjectCommitOutcome::Refused(
                ForecastObjectCommitRefusal::Allocation
            ))
        );
        assert!(!store.object_path(digest(1)).exists());
        assert!(!store.receipt_path(digest(2)).exists());

        let oversized = vec![0x1f; FORECAST_OBJECT_MAXIMUM_BYTES + 1];
        let mut admitted = FixtureAllocation::admitted();
        assert_eq!(
            store.commit(
                input(&prepared, &oversized, "2026-11-23T12:00:00.000Z"),
                &codec,
                &mut admitted,
            ),
            Ok(ForecastObjectCommitOutcome::Refused(
                ForecastObjectCommitRefusal::ObjectTooLarge
            ))
        );
        assert!(admitted.admissions.is_empty());
    }

    // verify existing content-address collisions and receipt races never overwrite
    #[test]
    fn preserves_existing_object_and_concurrent_first_receipt_bytes() {
        let root = DisposableRoot::new();
        let mut store = NativeForecastObjectStore::new(root.0.clone()).expect("store");
        let codec = FixtureCodec::default();
        let prepared = Prepared {
            object: 1,
            receipt: 2,
            version: ForecastObjectReceiptVersion::V1,
        };
        write_private(&store.object_path(digest(1)), b"different");
        let mut allocation = FixtureAllocation::admitted();
        assert_eq!(
            store.commit(
                input(&prepared, &[0x1f, 0x8b, 1], "2026-11-23T12:00:00.000Z"),
                &codec,
                &mut allocation,
            ),
            Err(ForecastObjectCommitError::ObjectCollision)
        );
        assert_eq!(
            fs::read(store.object_path(digest(1))).expect("retained object"),
            b"different"
        );

        let race_root = DisposableRoot::new();
        let mut race_store =
            NativeForecastObjectStore::new(race_root.0.clone()).expect("race store");
        let receipt = codec
            .encode_receipt(&prepared, "2026-11-23T11:00:00.000Z", digest(1), digest(2))
            .expect("race receipt");
        let mut race = RacingAllocation {
            receipt_bytes: receipt.bytes.clone(),
            receipt_path: race_store.receipt_path(digest(2)),
            observations: Vec::new(),
        };
        assert_eq!(
            race_store.commit(
                input(&prepared, &[0x1f, 0x8b, 1], "2026-11-23T12:00:00.000Z"),
                &codec,
                &mut race,
            ),
            Ok(ForecastObjectCommitOutcome::Duplicate)
        );
        assert_eq!(
            fs::read(race_store.receipt_path(digest(2))).expect("raced receipt"),
            receipt.bytes
        );
        assert_eq!(race.observations.len(), 2);
        assert!(race.observations[0].created);
        assert!(!race.observations[1].created);
    }
}
