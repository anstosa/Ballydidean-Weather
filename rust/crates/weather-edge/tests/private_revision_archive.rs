use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    AdjustmentRevisionBatchPublishReceipt, AdjustmentRevisionBatchPublishReceiptProjectionKindEnum,
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionProjectionKind,
    AdjustmentRevisionPublishReceipt, AdjustmentRevisionStageReceipt,
};
use weather_edge::{
    AdmittedPrivateRevisionProjection, PRIVATE_REVISION_BATCH_COMMIT_RECEIPT_MAXIMUM_BYTES,
    PrivateRevisionArchive, PrivateRevisionArchiveCodec, PrivateRevisionArchiveError,
    PrivateRevisionArchiveStorage, PrivateRevisionBatchPublishUnsigned,
    PrivateRevisionExclusiveWrite, PrivateRevisionProjectionDependency,
    PrivateRevisionProjectionShape, PrivateRevisionPublishInput, PrivateRevisionPublishReceipt,
    PrivateRevisionPublishUnsigned, PrivateRevisionReceiptCodec, PrivateRevisionStageUnsigned,
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, RevisionOnlinePayloadAddress,
    RevisionOnlinePayloadKind, RevisionOnlineSpoolError, RevisionOnlineSpoolStorage,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[path = "support/native_spool.rs"]
mod native_spool;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
use weather_edge::{
    AdjustmentMaintenanceGapReceipt, AdjustmentRevisionGapReceipt,
    NativeRevisionSpoolArtifactCodec, PermanentArchiveGapCodec, PermanentArchiveGapInput,
    RainControlProjectionBinding, RainControlStateArchiveCodec, RainControlStateWireCodec,
    RainMaintenanceControlStateGapReceipt, RevisionSpoolLeaseCodec,
};

const SINGLE_BYTES: &[u8] = b"single-projection";
const BATCH_BYTES: &[u8] = b"batch-projection";
const THIRD_BYTES: &[u8] = b"third-projection";

#[derive(Debug, Default)]
struct FixtureCodec {
    batch_publications: RefCell<Vec<(Vec<u8>, AdjustmentRevisionBatchPublishReceipt)>>,
    publications: RefCell<Vec<(Vec<u8>, AdjustmentRevisionPublishReceipt)>>,
    stages: RefCell<Vec<(Vec<u8>, AdjustmentRevisionStageReceipt)>>,
}

impl RevisionArchiveCanonicalCodec for FixtureCodec {
    /// admit the fixed fixture timestamps
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        text.ends_with(".000Z").then_some(1)
    }

    /// hash bytes with one deterministic dependency-free digest
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        let mut digest = [0_u8; 32];
        // mix every byte into one fixed digest lane
        for (index, byte) in bytes.iter().copied().enumerate() {
            let lane = index % digest.len();
            digest[lane] = digest[lane].wrapping_mul(31).wrapping_add(byte);
        }
        digest
    }
}

impl PrivateRevisionReceiptCodec for FixtureCodec {
    /// decode one previously encoded fixture stage receipt
    fn decode_stage_receipt(&self, bytes: &[u8]) -> Option<AdjustmentRevisionStageReceipt> {
        lookup_typed(&self.stages.borrow(), bytes)
    }

    /// encode the unsigned fixture stage receipt
    fn stage_unsigned_bytes(&self, value: PrivateRevisionStageUnsigned<'_>) -> Option<Vec<u8>> {
        Some(
            format!(
                "stage-u|{}|{}|{}|{}",
                value.durable_at,
                value.projection_identity_sha256,
                projection_kind_wire(value.projection_kind),
                value.projection_sha256,
            )
            .into_bytes(),
        )
    }

    /// encode and retain one complete fixture stage receipt
    fn stage_receipt_bytes(&self, value: &AdjustmentRevisionStageReceipt) -> Option<Vec<u8>> {
        let bytes = format!("stage|{}", value.stageReceiptSha256).into_bytes();
        self.stages
            .borrow_mut()
            .push((bytes.clone(), value.clone()));
        Some(bytes)
    }

    /// validate the closed fixture database receipt fields
    fn validate_revision_commit_receipt(&self, value: &AdjustmentRevisionCommitReceipt) -> bool {
        value.contractVersion == AdjustmentRevisionCommitReceiptContractVersionLiteral
            && value
                .archiveCommitOrdinal
                .parse::<u64>()
                .is_ok_and(|ordinal| ordinal > 0)
            && [
                &value.frontierSha256,
                &value.predecessorFrontierSha256,
                &value.projectionIdentitySha256,
                &value.projectionSha256,
                &value.receiptSha256,
                &value.stageReceiptSha256,
            ]
            .into_iter()
            .all(|digest| is_fixture_digest(digest))
    }

    /// authenticate the fixture receipt and frontier markers
    fn validate_revision_commit_receipt_identity(
        &self,
        value: &AdjustmentRevisionCommitReceipt,
    ) -> bool {
        let Some(ordinal) = value.archiveCommitOrdinal.parse::<u8>().ok() else {
            return false;
        };
        self.validate_revision_commit_receipt(value)
            && value.receiptSha256 == marker_digest(b'b'.wrapping_add(ordinal))
            && value.frontierSha256 == marker_digest(b'b'.wrapping_add(ordinal))
    }

    /// encode one fixture database receipt
    fn revision_commit_receipt_bytes(
        &self,
        value: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>> {
        Some(format!("commit|{}", value.receiptSha256).into_bytes())
    }

    /// encode one fixture frontier successor
    fn revision_successor_bytes(&self, value: &AdjustmentRevisionCommitReceipt) -> Option<Vec<u8>> {
        Some(
            format!(
                "successor|{}|{}",
                value.predecessorFrontierSha256, value.frontierSha256
            )
            .into_bytes(),
        )
    }

    /// encode one grouped fixture commit file
    fn revision_batch_commit_group_bytes(
        &self,
        values: &[AdjustmentRevisionCommitReceipt],
    ) -> Option<Vec<u8>> {
        Some(
            format!(
                "group|{}",
                values
                    .iter()
                    .map(|value| value.receiptSha256.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .into_bytes(),
        )
    }

    /// decode one previously encoded single publication
    fn decode_publish_receipt(&self, bytes: &[u8]) -> Option<AdjustmentRevisionPublishReceipt> {
        lookup_typed(&self.publications.borrow(), bytes)
    }

    /// encode the unsigned fixture single publication
    fn publish_unsigned_bytes(&self, value: PrivateRevisionPublishUnsigned<'_>) -> Option<Vec<u8>> {
        Some(
            format!(
                "publish-u|{}|{}|{}|{}|{}",
                value.committed_at,
                value.projection_identity_sha256,
                projection_kind_wire(value.projection_kind),
                value.projection_sha256,
                value.revision_receipt_sha256,
            )
            .into_bytes(),
        )
    }

    /// encode and retain one complete single publication
    fn publish_receipt_bytes(&self, value: &AdjustmentRevisionPublishReceipt) -> Option<Vec<u8>> {
        let bytes = format!("publish|{}", value.publishReceiptSha256).into_bytes();
        self.publications
            .borrow_mut()
            .push((bytes.clone(), value.clone()));
        Some(bytes)
    }

    /// decode one previously encoded grouped publication
    fn decode_batch_publish_receipt(
        &self,
        bytes: &[u8],
    ) -> Option<AdjustmentRevisionBatchPublishReceipt> {
        lookup_typed(&self.batch_publications.borrow(), bytes)
    }

    /// encode the unsigned fixture grouped publication
    fn batch_publish_unsigned_bytes(
        &self,
        value: PrivateRevisionBatchPublishUnsigned<'_>,
    ) -> Option<Vec<u8>> {
        Some(
            format!(
                "batch-u|{}|{}|{}|{}|{}",
                value.committed_at,
                value.projection_identity_sha256,
                batch_kind_wire(value.projection_kind),
                value.projection_sha256,
                value.revision_receipt_sha256s.join(","),
            )
            .into_bytes(),
        )
    }

    /// encode and retain one complete grouped publication
    fn batch_publish_receipt_bytes(
        &self,
        value: &AdjustmentRevisionBatchPublishReceipt,
    ) -> Option<Vec<u8>> {
        let bytes = format!("batch|{}", value.publishReceiptSha256).into_bytes();
        self.batch_publications
            .borrow_mut()
            .push((bytes.clone(), value.clone()));
        Some(bytes)
    }
}

impl PrivateRevisionArchiveCodec for FixtureCodec {
    /// decode the three supported fixture projections
    fn decode_projection(&self, bytes: &[u8]) -> Option<AdmittedPrivateRevisionProjection> {
        // retain the grouped two-row fixture
        if bytes == BATCH_BYTES {
            return Some(AdmittedPrivateRevisionProjection {
                projection_kind: AdjustmentRevisionProjectionKind::ActualBestMatch,
                shape: PrivateRevisionProjectionShape::Grouped { row_count: 2 },
                dependency: PrivateRevisionProjectionDependency::None,
            });
        }
        // retain both ordinary single fixtures
        if bytes == SINGLE_BYTES || bytes == THIRD_BYTES {
            return Some(AdmittedPrivateRevisionProjection {
                projection_kind: AdjustmentRevisionProjectionKind::NativeSource,
                shape: PrivateRevisionProjectionShape::Single,
                dependency: PrivateRevisionProjectionDependency::None,
            });
        }
        None
    }
}

#[derive(Debug, Default)]
struct FixtureStorage {
    batch_commits: Vec<(String, Vec<u8>)>,
    commits: Vec<(String, Vec<u8>)>,
    events: Vec<String>,
    permanent_gap: bool,
    projections: Vec<(RevisionOnlinePayloadAddress, Vec<u8>)>,
    publications: Vec<(String, Vec<u8>)>,
    stages: Vec<(String, Vec<u8>)>,
    successors: Vec<(String, Vec<u8>)>,
}

impl RevisionOnlineSpoolStorage for FixtureStorage {
    /// run one fixture operation under the shared lease marker
    fn with_revision_spool_lease<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, RevisionOnlineSpoolError>,
    ) -> Result<T, RevisionOnlineSpoolError> {
        self.events.push("lease".to_owned());
        operation(self)
    }

    /// read one optional retained fixture projection
    fn read_existing_payload(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        _maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError> {
        self.events.push("read-projection".to_owned());
        Ok(self
            .projections
            .iter()
            .find(|(candidate, _)| candidate == address)
            .map(|(_, bytes)| bytes.clone()))
    }

    /// list one fixture payload directory
    fn list_payload_names(
        &mut self,
        kind: RevisionOnlinePayloadKind,
    ) -> Result<Vec<String>, RevisionOnlineSpoolError> {
        self.events.push(format!("list-{}", kind.directory()));
        // expose only retained projection names
        if kind != RevisionOnlinePayloadKind::Projection {
            return Ok(Vec::new());
        }
        Ok(self
            .projections
            .iter()
            .map(|(address, _)| address.components().1.to_owned())
            .collect())
    }

    /// return one retained fixture payload size
    fn payload_file_length(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        _maximum_bytes: usize,
    ) -> Result<usize, RevisionOnlineSpoolError> {
        self.projections
            .iter()
            .find(|(candidate, _)| candidate == address)
            .map(|(_, bytes)| bytes.len())
            .ok_or(RevisionOnlineSpoolError::Storage)
    }
}

impl PrivateRevisionArchiveStorage for FixtureStorage {
    /// verify the fixture archive root
    fn ensure_revision_archive(&mut self) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push("ensure".to_owned());
        Ok(())
    }

    /// verify fixture projection dependencies
    fn verify_projection_dependencies(
        &mut self,
        _projection: &AdmittedPrivateRevisionProjection,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push("dependencies".to_owned());
        Ok(())
    }

    /// recheck fixture projection postconditions
    fn verify_projection_write_postcondition(
        &mut self,
        _projection: &AdmittedPrivateRevisionProjection,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push("postcondition".to_owned());
        Ok(())
    }

    /// retain one new fixture projection
    fn write_projection_exclusive(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push("write-projection".to_owned());
        self.projections.push((address.clone(), bytes.to_vec()));
        Ok(())
    }

    /// read one fixture stage receipt
    fn read_stage_receipt(
        &mut self,
        projection_identity_sha256: &str,
        _maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError> {
        self.events.push("read-stage".to_owned());
        Ok(find_bytes(&self.stages, projection_identity_sha256))
    }

    /// create one fixture stage receipt
    fn write_stage_receipt_exclusive(
        &mut self,
        projection_identity_sha256: &str,
        bytes: &[u8],
    ) -> Result<PrivateRevisionExclusiveWrite, RevisionOnlineSpoolError> {
        self.events.push("write-stage".to_owned());
        exclusive_write(&mut self.stages, projection_identity_sha256, bytes)
    }

    /// return the injected permanent gap state
    fn has_permanent_revision_gap(
        &mut self,
        _projection_identity_sha256: &str,
        _projection_kind: AdjustmentRevisionCommitReceiptProjectionKindEnum,
    ) -> Result<bool, RevisionOnlineSpoolError> {
        self.events.push("gap".to_owned());
        Ok(self.permanent_gap)
    }

    /// retain or verify one fixture commit receipt
    fn write_or_verify_commit_receipt(
        &mut self,
        frontier_sha256: &str,
        bytes: &[u8],
        _maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push("commit".to_owned());
        write_or_verify(&mut self.commits, frontier_sha256, bytes)
    }

    /// retain or verify one fixture successor
    fn write_or_verify_successor(
        &mut self,
        predecessor_frontier_sha256: &str,
        bytes: &[u8],
        _maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError> {
        self.events.push("successor".to_owned());
        write_or_verify(&mut self.successors, predecessor_frontier_sha256, bytes)
    }

    /// retain or verify one fixture grouped commit file
    fn write_or_verify_batch_commit_group(
        &mut self,
        projection_identity_sha256: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError> {
        assert_eq!(
            maximum_bytes,
            PRIVATE_REVISION_BATCH_COMMIT_RECEIPT_MAXIMUM_BYTES
        );
        self.events.push("batch-commit".to_owned());
        write_or_verify(&mut self.batch_commits, projection_identity_sha256, bytes)
    }

    /// read one fixture publication receipt
    fn read_publish_receipt(
        &mut self,
        projection_identity_sha256: &str,
        _maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError> {
        self.events.push("read-publication".to_owned());
        Ok(find_bytes(&self.publications, projection_identity_sha256))
    }

    /// create one fixture publication receipt
    fn write_publish_receipt_exclusive(
        &mut self,
        projection_identity_sha256: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<PrivateRevisionExclusiveWrite, RevisionOnlineSpoolError> {
        assert!(bytes.len() <= maximum_bytes);
        self.events.push("write-publication".to_owned());
        exclusive_write(&mut self.publications, projection_identity_sha256, bytes)
    }
}

#[derive(Debug)]
struct FixtureClock {
    values: VecDeque<String>,
}

impl FixtureClock {
    /// create one ordered fixture clock
    fn new(values: &[&str]) -> Self {
        Self {
            values: values.iter().map(|value| (*value).to_owned()).collect(),
        }
    }
}

impl RevisionArchiveCanonicalClock for FixtureClock {
    /// return the next fixture archive instant
    fn canonical_now(&mut self) -> Option<String> {
        self.values.pop_front()
    }
}

/// return one cloned typed value for exact encoded bytes
fn lookup_typed<T: Clone>(entries: &[(Vec<u8>, T)], bytes: &[u8]) -> Option<T> {
    entries
        .iter()
        .find(|(candidate, _)| candidate == bytes)
        .map(|(_, value)| value.clone())
}

/// return one marker digest in the lowercase wire alphabet
fn marker_digest(byte: u8) -> String {
    char::from(byte).to_string().repeat(64)
}

/// accept one lowercase fixture digest
fn is_fixture_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// map one projection kind to the fixture wire value
fn projection_kind_wire(value: AdjustmentRevisionProjectionKind) -> &'static str {
    match value {
        AdjustmentRevisionProjectionKind::ActualBestMatch => "actual_best_match",
        AdjustmentRevisionProjectionKind::NativeSource => "native_source",
        AdjustmentRevisionProjectionKind::RainGateInput => "rain_gate_input",
        AdjustmentRevisionProjectionKind::TargetRevision => "target_revision",
    }
}

/// map one grouped projection kind to the fixture wire value
fn batch_kind_wire(value: AdjustmentRevisionBatchPublishReceiptProjectionKindEnum) -> &'static str {
    match value {
        AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::ActualBestMatch => {
            "actual_best_match"
        }
        AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::TargetRevision => {
            "target_revision"
        }
    }
}

/// return one retained fixture byte value
fn find_bytes(entries: &[(String, Vec<u8>)], key: &str) -> Option<Vec<u8>> {
    entries
        .iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, bytes)| bytes.clone())
}

/// create one immutable fixture value without replacing an existing entry
fn exclusive_write(
    entries: &mut Vec<(String, Vec<u8>)>,
    key: &str,
    bytes: &[u8],
) -> Result<PrivateRevisionExclusiveWrite, RevisionOnlineSpoolError> {
    // retain the existing bytes for one racing writer outcome
    if entries.iter().any(|(candidate, _)| candidate == key) {
        return Ok(PrivateRevisionExclusiveWrite::AlreadyExists);
    }
    entries.push((key.to_owned(), bytes.to_vec()));
    Ok(PrivateRevisionExclusiveWrite::Written)
}

/// retain or verify one immutable fixture value
fn write_or_verify(
    entries: &mut Vec<(String, Vec<u8>)>,
    key: &str,
    bytes: &[u8],
) -> Result<(), RevisionOnlineSpoolError> {
    // require byte identity on an existing key
    if let Some((_, existing)) = entries.iter().find(|(candidate, _)| candidate == key) {
        return (existing == bytes)
            .then_some(())
            .ok_or(RevisionOnlineSpoolError::IdentityConflict);
    }
    entries.push((key.to_owned(), bytes.to_vec()));
    Ok(())
}

/// create one authenticated fixture database receipt
fn commit_receipt(
    ordinal: u8,
    predecessor: &str,
    projection_identity: &str,
    projection_kind: AdjustmentRevisionCommitReceiptProjectionKindEnum,
    stage: &AdjustmentRevisionStageReceipt,
) -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: ordinal.to_string(),
        archiveCommittedAt: format!("2026-11-23T12:0{ordinal}:00.000Z"),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: marker_digest(b'b'.wrapping_add(ordinal)),
        predecessorFrontierSha256: predecessor.to_owned(),
        projectionIdentitySha256: projection_identity.to_owned(),
        projectionKind: projection_kind,
        projectionSha256: projection_identity.to_owned(),
        receiptSha256: marker_digest(b'b'.wrapping_add(ordinal)),
        stageReceiptSha256: stage.stageReceiptSha256.clone(),
    }
}

/// render one fixture digest in lowercase hexadecimal
fn digest_text(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    // emit each fixture digest nibble
    for byte in digest {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

/// run the complete ordinary single and grouped fixture
fn run_fixture() -> (String, FixtureStorage) {
    let clock = FixtureClock::new(&[
        "2026-11-23T12:10:00.000Z",
        "2026-11-23T14:10:00.000Z",
        "2026-11-23T16:10:00.000Z",
        "2026-11-23T17:10:00.000Z",
    ]);
    let codec = FixtureCodec::default();
    let single_identity = digest_text(codec.sha256(SINGLE_BYTES));
    let batch_identity = digest_text(codec.sha256(BATCH_BYTES));
    let mut archive = PrivateRevisionArchive::new(FixtureStorage::default(), codec, clock);

    let single_stage = archive.stage(SINGLE_BYTES).expect("single stage");
    assert_eq!(
        archive.stage(SINGLE_BYTES).expect("single stage retry"),
        single_stage
    );
    let single_revision = commit_receipt(
        1,
        &marker_digest(b'a'),
        &single_identity,
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource,
        &single_stage,
    );
    let PrivateRevisionPublishReceipt::Single(single_publish) = archive
        .publish(PrivateRevisionPublishInput::Single {
            revision_receipt: single_revision.clone(),
            stage_receipt: single_stage.clone(),
        })
        .expect("single publish")
    else {
        panic!("expected single publication")
    };
    assert_eq!(
        archive
            .publish(PrivateRevisionPublishInput::Single {
                revision_receipt: single_revision.clone(),
                stage_receipt: single_stage.clone(),
            })
            .expect("single publish retry"),
        PrivateRevisionPublishReceipt::Single(single_publish.clone())
    );

    let batch_stage = archive.stage(BATCH_BYTES).expect("batch stage");
    let first = commit_receipt(
        2,
        &single_revision.frontierSha256,
        &batch_identity,
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
        &batch_stage,
    );
    let second = commit_receipt(
        3,
        &first.frontierSha256,
        &batch_identity,
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch,
        &batch_stage,
    );
    let PrivateRevisionPublishReceipt::Batch(batch_publish) = archive
        .publish(PrivateRevisionPublishInput::Batch {
            revision_receipts: vec![first.clone(), second.clone()],
            stage_receipt: batch_stage.clone(),
        })
        .expect("batch publish")
    else {
        panic!("expected grouped publication")
    };
    assert_eq!(
        archive
            .publish(PrivateRevisionPublishInput::Batch {
                revision_receipts: vec![first.clone(), second.clone()],
                stage_receipt: batch_stage.clone(),
            })
            .expect("batch publish retry"),
        PrivateRevisionPublishReceipt::Batch(batch_publish.clone())
    );

    let signature = format!(
        concat!(
            "single-stage|{}|{}|{}|{}|{}\n",
            "single-publish|{}|{}|{}|{}|{}\n",
            "batch-stage|{}|{}|{}|{}\n",
            "batch-publish|{}|{}|{}|{}|{}|{}\n",
        ),
        projection_kind_wire(single_stage.projectionKind),
        single_stage.durableAt,
        single_stage.projectionIdentitySha256 == single_identity,
        single_stage.projectionSha256 == single_identity,
        single_stage.stageReceiptSha256.len(),
        projection_kind_wire(single_publish.projectionKind),
        single_publish.committedAt,
        single_publish.projectionIdentitySha256 == single_identity,
        single_publish.revisionReceiptSha256 == single_revision.receiptSha256,
        single_publish.publishReceiptSha256.len(),
        projection_kind_wire(batch_stage.projectionKind),
        batch_stage.durableAt,
        batch_stage.projectionIdentitySha256 == batch_identity,
        batch_stage.stageReceiptSha256.len(),
        batch_kind_wire(batch_publish.projectionKind),
        batch_publish.committedAt,
        batch_publish.projectionIdentitySha256 == batch_identity,
        batch_publish.revisionReceiptSha256s == vec![first.receiptSha256, second.receiptSha256],
        batch_publish.revisionReceiptSha256s.len(),
        batch_publish.publishReceiptSha256.len(),
    );
    (signature, archive.into_storage())
}

// verifies ordinary stage and publication parity against the actual Node store
#[test]
fn matches_actual_node_single_and_grouped_revision_archive() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/private-revision-archive-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run private revision archive Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let (rust, storage) = run_fixture();
    assert_eq!(rust, node);
    assert_eq!(storage.projections.len(), 2);
    assert_eq!(storage.commits.len(), 1);
    assert_eq!(storage.successors.len(), 1);
    assert_eq!(storage.batch_commits.len(), 1);
    assert_eq!(storage.publications.len(), 2);
}

// verifies custody ordering and permanent-gap rejection stay operationally closed
#[test]
fn preserves_write_order_and_ordinary_rejections() {
    let (_, storage) = run_fixture();
    let single_commit = storage
        .events
        .iter()
        .position(|event| event == "commit")
        .expect("single commit event");
    let single_successor = storage
        .events
        .iter()
        .position(|event| event == "successor")
        .expect("single successor event");
    let single_publication = storage
        .events
        .iter()
        .position(|event| event == "write-publication")
        .expect("single publication event");
    assert!(single_commit < single_successor && single_successor < single_publication);

    let codec = FixtureCodec::default();
    let identity = digest_text(codec.sha256(SINGLE_BYTES));
    let storage = FixtureStorage {
        permanent_gap: true,
        ..FixtureStorage::default()
    };
    let mut archive = PrivateRevisionArchive::new(
        storage,
        codec,
        FixtureClock::new(&["2026-11-23T12:10:00.000Z"]),
    );
    let stage = archive.stage(SINGLE_BYTES).expect("stage before gap");
    let revision = commit_receipt(
        1,
        &marker_digest(b'a'),
        &identity,
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource,
        &stage,
    );
    let error = archive
        .publish(PrivateRevisionPublishInput::Single {
            revision_receipt: revision,
            stage_receipt: stage,
        })
        .expect_err("permanent gap rejects publication");
    assert_eq!(error, PrivateRevisionArchiveError::PermanentGap);
}

// verifies the shared two-slot capacity condition remains distinct
#[test]
fn preserves_shared_projection_capacity_outcome() {
    let mut archive = PrivateRevisionArchive::new(
        FixtureStorage::default(),
        FixtureCodec::default(),
        FixtureClock::new(&["2026-11-23T12:10:00.000Z", "2026-11-23T13:10:00.000Z"]),
    );
    archive.stage(SINGLE_BYTES).expect("first projection");
    archive.stage(BATCH_BYTES).expect("second projection");
    let error = archive
        .stage(THIRD_BYTES)
        .expect_err("third projection exceeds shared slots");
    assert_eq!(error, PrivateRevisionArchiveError::SpoolFull);
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
struct NativeFixtureCodec {
    lease: native_spool::NodeLeaseCodec,
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl RevisionSpoolLeaseCodec for NativeFixtureCodec {
    /// delegate complete fixture lease encoding to the Node oracle
    fn encode_canonical_lease(&self, milliseconds: i64, token: &str) -> Option<Vec<u8>> {
        self.lease.encode_canonical_lease(milliseconds, token)
    }

    /// delegate complete lease parsing to the Node oracle
    fn validated_lease_expiry(&self, bytes: &[u8]) -> Option<i64> {
        self.lease.validated_lease_expiry(bytes)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl RevisionArchiveCanonicalCodec for NativeFixtureCodec {
    /// admit the fixed native fixture timestamps
    fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
        text.ends_with(".000Z").then_some(1)
    }

    /// reuse the deterministic fixture digest for unused artifact parsing
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        FixtureCodec::default().sha256(bytes)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl RainControlStateWireCodec for NativeFixtureCodec {
    /// This fixture does not submit monthly rain control states.
    fn decode_control_json(&self, _: &[u8]) -> Option<weather_contracts::JsonValue> {
        None
    }

    /// No monthly state document is serialized in this fixture.
    fn encode_control_ordered_json(&self, _: &weather_contracts::JsonValue) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// No monthly state self hash is serialized in this fixture.
    fn encode_control_sorted_json(&self, _: &weather_contracts::JsonValue) -> Option<Vec<u8>> {
        unreachable!()
    }

    /// reject unused monthly stage receipt bytes
    fn decode_stage_receipt(
        &self,
        _bytes: &[u8],
    ) -> Option<weather_contracts::RainMaintenanceControlStateStageReceipt> {
        None
    }

    /// reject unused monthly gap receipt bytes
    fn decode_gap_receipt(&self, _bytes: &[u8]) -> Option<RainMaintenanceControlStateGapReceipt> {
        None
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl RainControlStateArchiveCodec for NativeFixtureCodec {
    /// classify fixture projections as unrelated to monthly state
    fn decode_projection(&self, _bytes: &[u8]) -> Option<RainControlProjectionBinding> {
        Some(RainControlProjectionBinding::Other)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl PermanentArchiveGapCodec for NativeFixtureCodec {
    /// reject unused gap locator encoding in this revision fixture
    fn encode_gap_key(&self, _input: PermanentArchiveGapInput<'_>) -> Option<Vec<u8>> {
        None
    }

    /// reject unused gap receipt encoding in this revision fixture
    fn encode_gap_receipt(
        &self,
        _input: PermanentArchiveGapInput<'_>,
        _recorded_at: &str,
        _digest: Option<&str>,
    ) -> Option<Vec<u8>> {
        None
    }

    /// reject unused revision gap bytes in an empty fixture directory
    fn accepted_revision_gap(&self, _bytes: &[u8]) -> Option<AdjustmentRevisionGapReceipt> {
        None
    }

    /// reject unused shadow gap bytes in an empty fixture directory
    fn accepted_shadow_gap(&self, _bytes: &[u8]) -> Option<AdjustmentMaintenanceGapReceipt> {
        None
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl NativeRevisionSpoolArtifactCodec for NativeFixtureCodec {
    /// reject unused monthly ordinal parsing in this revision-only fixture
    fn admitted_rain_control_ordinal_sha256(&self, _bytes: &[u8]) -> Option<String> {
        None
    }
}

// proves the concrete Linux spool durably stages and publishes one ordinary revision
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn native_spool_durably_stages_and_publishes_single_revision() {
    use native_spool::TempSpool;
    use weather_edge::NativeRevisionSpool;

    let temp = TempSpool::new();
    let storage_codec = NativeFixtureCodec {
        lease: native_spool::NodeLeaseCodec,
    };
    let spool = NativeRevisionSpool::new(temp.path().to_path_buf(), storage_codec)
        .expect("native revision spool");
    let archive_codec = FixtureCodec::default();
    let identity = digest_text(archive_codec.sha256(SINGLE_BYTES));
    let mut archive = PrivateRevisionArchive::new(
        spool,
        archive_codec,
        FixtureClock::new(&["2026-11-23T12:10:00.000Z", "2026-11-23T14:10:00.000Z"]),
    );
    let stage = archive.stage(SINGLE_BYTES).expect("native stage");
    let revision = commit_receipt(
        1,
        &marker_digest(b'a'),
        &identity,
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource,
        &stage,
    );
    let publication = archive
        .publish(PrivateRevisionPublishInput::Single {
            revision_receipt: revision.clone(),
            stage_receipt: stage,
        })
        .expect("native publication");
    assert!(matches!(
        publication,
        PrivateRevisionPublishReceipt::Single(_)
    ));

    let expected = [
        (
            "revision-projections",
            format!("sha256-{identity}.json"),
            SINGLE_BYTES,
        ),
        (
            "revision-stage-receipts",
            format!("sha256-{identity}.json"),
            b"stage|".as_slice(),
        ),
        (
            "revision-commit-receipts",
            format!("sha256-{}.json", revision.frontierSha256),
            b"commit|".as_slice(),
        ),
        (
            "revision-frontier-successors",
            format!("sha256-{}.json", revision.predecessorFrontierSha256),
            b"successor|".as_slice(),
        ),
        (
            "revision-publish-receipts",
            format!("sha256-{identity}.json"),
            b"publish|".as_slice(),
        ),
    ];
    // verify each exact fixed-directory member and its fixture byte prefix
    for (directory, name, prefix) in expected {
        let bytes = std::fs::read(temp.path().join(directory).join(name)).expect("native member");
        assert!(bytes.starts_with(prefix));
        assert_eq!(
            std::fs::read_dir(temp.path().join(directory))
                .expect("native directory")
                .count(),
            1
        );
    }
    assert!(!temp.path().join("revision-spool-locks/spool.lock").exists());
}
