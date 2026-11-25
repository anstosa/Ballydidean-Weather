use weather_contracts::{
    AdjustmentRevisionBatchPublishReceipt, AdjustmentRevisionBatchPublishReceiptCommittedLiteral,
    AdjustmentRevisionBatchPublishReceiptContractVersionLiteral,
    AdjustmentRevisionBatchPublishReceiptProjectionKindEnum, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionProjectionKind,
    AdjustmentRevisionPublishReceipt, AdjustmentRevisionPublishReceiptCommittedLiteral,
    AdjustmentRevisionPublishReceiptContractVersionLiteral, AdjustmentRevisionStageReceipt,
    AdjustmentRevisionStageReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptDurableLiteral,
};

use crate::private_archive_service::{PrivateRevisionPublishInput, PrivateRevisionPublishReceipt};
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, instant, lower_hex,
};
use crate::revision_online_spool::{
    RevisionOnlinePayload, RevisionOnlinePayloadAddress, RevisionOnlinePayloadAdmission,
    RevisionOnlinePayloadKind, RevisionOnlineSpoolError, RevisionOnlineSpoolStorage, admit_payload,
};

pub const PRIVATE_REVISION_STAGE_RECEIPT_MAXIMUM_BYTES: usize = 4_096;
pub const PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES: usize = 4_096;
pub const PRIVATE_REVISION_BATCH_PUBLISH_RECEIPT_MAXIMUM_BYTES: usize = 32 * 1_024;
pub const PRIVATE_REVISION_COMMIT_RECEIPT_MAXIMUM_BYTES: usize = 4_096;
pub const PRIVATE_REVISION_BATCH_COMMIT_RECEIPT_MAXIMUM_BYTES: usize = 256 * 1_024;
pub const PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS: usize = 168;

/// one projection publication shape admitted by the exact producer parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateRevisionProjectionShape {
    Single,
    Grouped { row_count: usize },
}

/// one optional projection dependency retained outside the projection body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivateRevisionProjectionDependency {
    None,
    RainControlState {
        ordinal_artifact_sha256: String,
        state_sha256: String,
        state_stage_receipt_sha256: String,
    },
}

/// one exact parsed projection projected to archive-relevant metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedPrivateRevisionProjection {
    pub projection_kind: AdjustmentRevisionProjectionKind,
    pub shape: PrivateRevisionProjectionShape,
    pub dependency: PrivateRevisionProjectionDependency,
}

/// one unsigned stage receipt passed to the canonical JSON owner.
#[derive(Debug, Clone, Copy)]
pub struct PrivateRevisionStageUnsigned<'a> {
    pub durable_at: &'a str,
    pub projection_identity_sha256: &'a str,
    pub projection_kind: AdjustmentRevisionProjectionKind,
    pub projection_sha256: &'a str,
}

/// one unsigned single publication passed to the canonical JSON owner.
#[derive(Debug, Clone, Copy)]
pub struct PrivateRevisionPublishUnsigned<'a> {
    pub committed_at: &'a str,
    pub projection_identity_sha256: &'a str,
    pub projection_kind: AdjustmentRevisionProjectionKind,
    pub projection_sha256: &'a str,
    pub revision_receipt_sha256: &'a str,
}

/// one unsigned grouped publication passed to the canonical JSON owner.
#[derive(Debug, Clone, Copy)]
pub struct PrivateRevisionBatchPublishUnsigned<'a> {
    pub committed_at: &'a str,
    pub projection_identity_sha256: &'a str,
    pub projection_kind: AdjustmentRevisionBatchPublishReceiptProjectionKindEnum,
    pub projection_sha256: &'a str,
    pub revision_receipt_sha256s: &'a [String],
}

/// exact canonical receipt and identity operations owned by the runtime codec.
pub trait PrivateRevisionReceiptCodec: RevisionArchiveCanonicalCodec {
    /// decode one exact canonical stage receipt.
    fn decode_stage_receipt(&self, bytes: &[u8]) -> Option<AdjustmentRevisionStageReceipt>;

    /// encode the unsigned stage receipt fields canonically.
    fn stage_unsigned_bytes(&self, value: PrivateRevisionStageUnsigned<'_>) -> Option<Vec<u8>>;

    /// encode one complete stage receipt canonically.
    fn stage_receipt_bytes(&self, value: &AdjustmentRevisionStageReceipt) -> Option<Vec<u8>>;

    /// validate the closed database receipt fields before storage access.
    fn validate_revision_commit_receipt(&self, value: &AdjustmentRevisionCommitReceipt) -> bool;

    /// recompute the database receipt and frontier identities.
    fn validate_revision_commit_receipt_identity(
        &self,
        value: &AdjustmentRevisionCommitReceipt,
    ) -> bool;

    /// encode one authenticated database receipt canonically.
    fn revision_commit_receipt_bytes(
        &self,
        value: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>>;

    /// encode one frontier successor bound to the authenticated receipt.
    fn revision_successor_bytes(&self, value: &AdjustmentRevisionCommitReceipt) -> Option<Vec<u8>>;

    /// encode one grouped commit file with all ordered successors.
    fn revision_batch_commit_group_bytes(
        &self,
        values: &[AdjustmentRevisionCommitReceipt],
    ) -> Option<Vec<u8>>;

    /// decode one exact canonical single publication receipt.
    fn decode_publish_receipt(&self, bytes: &[u8]) -> Option<AdjustmentRevisionPublishReceipt>;

    /// encode the unsigned single publication fields canonically.
    fn publish_unsigned_bytes(&self, value: PrivateRevisionPublishUnsigned<'_>) -> Option<Vec<u8>>;

    /// encode one complete single publication receipt canonically.
    fn publish_receipt_bytes(&self, value: &AdjustmentRevisionPublishReceipt) -> Option<Vec<u8>>;

    /// decode one exact canonical grouped publication receipt.
    fn decode_batch_publish_receipt(
        &self,
        bytes: &[u8],
    ) -> Option<AdjustmentRevisionBatchPublishReceipt>;

    /// encode the unsigned grouped publication fields canonically.
    fn batch_publish_unsigned_bytes(
        &self,
        value: PrivateRevisionBatchPublishUnsigned<'_>,
    ) -> Option<Vec<u8>>;

    /// encode one complete grouped publication receipt canonically.
    fn batch_publish_receipt_bytes(
        &self,
        value: &AdjustmentRevisionBatchPublishReceipt,
    ) -> Option<Vec<u8>>;
}

/// producer-only projection admission layered over the shared receipt codec.
pub trait PrivateRevisionArchiveCodec: PrivateRevisionReceiptCodec {
    /// parse and validate one complete canonical projection body.
    fn decode_projection(&self, bytes: &[u8]) -> Option<AdmittedPrivateRevisionProjection>;
}

/// one exclusive metadata publication result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateRevisionExclusiveWrite {
    Written,
    AlreadyExists,
}

/// native storage operations below the typed revision archive owner.
pub trait PrivateRevisionArchiveStorage: RevisionOnlineSpoolStorage {
    /// verify the existing private root and its fixed children.
    fn ensure_revision_archive(&mut self) -> Result<(), RevisionOnlineSpoolError>;

    /// verify external state and receipts required by the parsed projection.
    fn verify_projection_dependencies(
        &mut self,
        projection: &AdmittedPrivateRevisionProjection,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// recheck permanent projection preconditions under the spool lease.
    fn verify_projection_write_postcondition(
        &mut self,
        projection: &AdmittedPrivateRevisionProjection,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// publish one new immutable projection under its content address.
    fn write_projection_exclusive(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// read one optional bounded stage receipt by projection identity.
    fn read_stage_receipt(
        &mut self,
        projection_identity_sha256: &str,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError>;

    /// create one stage receipt without replacing a racing writer.
    fn write_stage_receipt_exclusive(
        &mut self,
        projection_identity_sha256: &str,
        bytes: &[u8],
    ) -> Result<PrivateRevisionExclusiveWrite, RevisionOnlineSpoolError>;

    /// report one permanent gap bound to this exact projection and kind.
    fn has_permanent_revision_gap(
        &mut self,
        projection_identity_sha256: &str,
        projection_kind: AdjustmentRevisionCommitReceiptProjectionKindEnum,
    ) -> Result<bool, RevisionOnlineSpoolError>;

    /// retain or verify one authenticated single database receipt.
    fn write_or_verify_commit_receipt(
        &mut self,
        frontier_sha256: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// retain or verify one authenticated frontier successor.
    fn write_or_verify_successor(
        &mut self,
        predecessor_frontier_sha256: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// retain or verify one complete grouped database receipt file.
    fn write_or_verify_batch_commit_group(
        &mut self,
        projection_identity_sha256: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// read one optional single or grouped publication receipt.
    fn read_publish_receipt(
        &mut self,
        projection_identity_sha256: &str,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError>;

    /// create one publication receipt without replacing a racing writer.
    fn write_publish_receipt_exclusive(
        &mut self,
        projection_identity_sha256: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<PrivateRevisionExclusiveWrite, RevisionOnlineSpoolError>;
}

/// closed revision archive failures surfaced to the private service adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateRevisionArchiveError {
    Input,
    Binding,
    PermanentGap,
    StoredReceipt,
    Clock,
    Storage,
    SpoolFull,
}

impl From<RevisionOnlineSpoolError> for PrivateRevisionArchiveError {
    /// collapse native spool failures without losing the retryable capacity outcome.
    fn from(value: RevisionOnlineSpoolError) -> Self {
        // preserve only the public retryable capacity class
        match value {
            RevisionOnlineSpoolError::Capacity => Self::SpoolFull,
            RevisionOnlineSpoolError::Input => Self::Input,
            RevisionOnlineSpoolError::IdentityConflict => Self::Binding,
            RevisionOnlineSpoolError::Storage | RevisionOnlineSpoolError::Census => Self::Storage,
        }
    }
}

/// typed owner for ordinary private revision staging and publication.
pub struct PrivateRevisionArchive<Storage, Codec, Clock> {
    storage: Storage,
    codec: Codec,
    clock: Clock,
}

impl<Storage, Codec, Clock> PrivateRevisionArchive<Storage, Codec, Clock>
where
    Storage: PrivateRevisionArchiveStorage,
    Codec: PrivateRevisionArchiveCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// bind one archive owner to its injected native boundaries.
    pub const fn new(storage: Storage, codec: Codec, clock: Clock) -> Self {
        Self {
            storage,
            codec,
            clock,
        }
    }

    /// return the storage handle to the owning archive process.
    pub fn into_storage(self) -> Storage {
        self.storage
    }

    /// stage one canonical projection and retain its first durable receipt.
    pub fn stage(
        &mut self,
        bytes: &[u8],
    ) -> Result<AdjustmentRevisionStageReceipt, PrivateRevisionArchiveError> {
        // reject invalid bounds before invoking the producer parser
        if bytes.len() < 2 || bytes.len() > RevisionOnlinePayloadKind::Projection.maximum_bytes() {
            return Err(PrivateRevisionArchiveError::Input);
        }
        let projection = self
            .codec
            .decode_projection(bytes)
            .ok_or(PrivateRevisionArchiveError::Input)?;
        let projection_identity_sha256 = lower_hex(self.codec.sha256(bytes));
        let address = RevisionOnlinePayloadAddress::from_sha256(
            RevisionOnlinePayloadKind::Projection,
            &projection_identity_sha256,
        )
        .ok_or(PrivateRevisionArchiveError::Input)?;
        let payload = RevisionOnlinePayload::new(&address, bytes)?;
        self.storage.ensure_revision_archive()?;
        self.storage.verify_projection_dependencies(&projection)?;
        self.storage.with_revision_spool_lease(
            // retain the permanent-gap check, census, and write under one lease
            |storage| {
                storage.verify_projection_write_postcondition(&projection)?;
                let admission = admit_payload(storage, &payload)?;
                // write only a fresh content address after complete admission
                if admission == RevisionOnlinePayloadAdmission::Fresh {
                    storage.write_projection_exclusive(&address, bytes)?;
                }
                Ok(())
            },
        )?;
        stage_receipt(
            &mut self.storage,
            &self.codec,
            &mut self.clock,
            &projection_identity_sha256,
            projection.projection_kind,
        )
    }

    /// publish one authenticated single or grouped database result.
    pub fn publish(
        &mut self,
        input: PrivateRevisionPublishInput,
    ) -> Result<PrivateRevisionPublishReceipt, PrivateRevisionArchiveError> {
        // preserve the exact additive single-versus-grouped grammar
        match input {
            PrivateRevisionPublishInput::Single {
                revision_receipt,
                stage_receipt,
            } => self
                .publish_single(revision_receipt, stage_receipt)
                .map(PrivateRevisionPublishReceipt::Single),
            PrivateRevisionPublishInput::Batch {
                revision_receipts,
                stage_receipt,
            } => self
                .publish_batch(revision_receipts, stage_receipt)
                .map(PrivateRevisionPublishReceipt::Batch),
        }
    }

    /// publish one staged body after its exact authenticated database receipt.
    fn publish_single(
        &mut self,
        revision: AdjustmentRevisionCommitReceipt,
        supplied_stage: AdjustmentRevisionStageReceipt,
    ) -> Result<AdjustmentRevisionPublishReceipt, PrivateRevisionArchiveError> {
        // reject structural receipt failures before storage access
        if !self.codec.validate_revision_commit_receipt(&revision) {
            return Err(PrivateRevisionArchiveError::Input);
        }
        self.storage.ensure_revision_archive()?;
        // retain permanent failure classification before reading the stage
        if self.storage.has_permanent_revision_gap(
            &revision.projectionIdentitySha256,
            revision.projectionKind,
        )? {
            return Err(PrivateRevisionArchiveError::PermanentGap);
        }
        let projection_kind = commit_projection_kind(revision.projectionKind)
            .ok_or(PrivateRevisionArchiveError::Binding)?;
        let stored_stage = read_bound_stage_receipt(
            &mut self.storage,
            &self.codec,
            &revision.projectionIdentitySha256,
            projection_kind,
            &revision.projectionSha256,
        )?;
        // bind the caller stage and database receipt to one staged body
        if supplied_stage != stored_stage
            || revision.stageReceiptSha256 != stored_stage.stageReceiptSha256
            || revision.projectionSha256 != revision.projectionIdentitySha256
        {
            return Err(PrivateRevisionArchiveError::Binding);
        }
        // authenticate the database-issued receipt only after stage binding
        if !self
            .codec
            .validate_revision_commit_receipt_identity(&revision)
        {
            return Err(PrivateRevisionArchiveError::Binding);
        }
        persist_single_commit(&mut self.storage, &self.codec, &revision)?;
        single_publish_receipt(
            &mut self.storage,
            &self.codec,
            &mut self.clock,
            &revision,
            projection_kind,
        )
    }

    /// publish one complete grouped body and its ordered global receipt chain.
    fn publish_batch(
        &mut self,
        receipts: Vec<AdjustmentRevisionCommitReceipt>,
        supplied_stage: AdjustmentRevisionStageReceipt,
    ) -> Result<AdjustmentRevisionBatchPublishReceipt, PrivateRevisionArchiveError> {
        // retain one bounded nonempty receipt group
        if receipts.is_empty() || receipts.len() > PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS {
            return Err(PrivateRevisionArchiveError::Input);
        }
        // authenticate every receipt before using shared chain coordinates
        if !receipts.iter().all(|receipt| {
            self.codec
                .validate_revision_commit_receipt_identity(receipt)
        }) {
            return Err(PrivateRevisionArchiveError::Binding);
        }
        let first = receipts.first().ok_or(PrivateRevisionArchiveError::Input)?;
        self.storage.ensure_revision_archive()?;
        let projection_kind = commit_projection_kind(first.projectionKind)
            .ok_or(PrivateRevisionArchiveError::Binding)?;
        let batch_kind =
            batch_projection_kind(projection_kind).ok_or(PrivateRevisionArchiveError::Binding)?;
        let stored_stage = read_bound_stage_receipt(
            &mut self.storage,
            &self.codec,
            &first.projectionIdentitySha256,
            projection_kind,
            &first.projectionSha256,
        )?;
        let address = RevisionOnlinePayloadAddress::from_sha256(
            RevisionOnlinePayloadKind::Projection,
            &first.projectionIdentitySha256,
        )
        .ok_or(PrivateRevisionArchiveError::Binding)?;
        let projection_bytes = self
            .storage
            .read_existing_payload(
                &address,
                RevisionOnlinePayloadKind::Projection.maximum_bytes(),
            )?
            .ok_or(PrivateRevisionArchiveError::Binding)?;
        let projection = self
            .codec
            .decode_projection(&projection_bytes)
            .ok_or(PrivateRevisionArchiveError::Binding)?;
        // restrict grouped publication to one exact parsed body and row count
        if projection.projection_kind != projection_kind
            || projection.shape
                != (PrivateRevisionProjectionShape::Grouped {
                    row_count: receipts.len(),
                })
            || supplied_stage != stored_stage
        {
            return Err(PrivateRevisionArchiveError::Binding);
        }
        // bind every receipt to one body, stage, and direct global chain
        if !valid_batch_chain(&receipts, first, &stored_stage) {
            return Err(PrivateRevisionArchiveError::Binding);
        }
        if self
            .storage
            .has_permanent_revision_gap(&first.projectionIdentitySha256, first.projectionKind)?
        {
            return Err(PrivateRevisionArchiveError::PermanentGap);
        }
        let codec = &self.codec;
        let clock = &mut self.clock;
        self.storage
            .with_revision_spool_lease(
                // retain grouped commit bytes before the publication marker
                |storage| {
                    let group_bytes = codec
                        .revision_batch_commit_group_bytes(&receipts)
                        .ok_or(RevisionOnlineSpoolError::Input)?;
                    storage.write_or_verify_batch_commit_group(
                        &first.projectionIdentitySha256,
                        &group_bytes,
                        PRIVATE_REVISION_BATCH_COMMIT_RECEIPT_MAXIMUM_BYTES,
                    )?;
                    batch_publish_receipt(storage, codec, clock, &receipts, batch_kind)
                        .map_err(private_error_to_spool)
                },
            )
            .map_err(PrivateRevisionArchiveError::from)
    }
}

/// load or create the first durable stage receipt after projection persistence.
fn stage_receipt<Storage, Codec, Clock>(
    storage: &mut Storage,
    codec: &Codec,
    clock: &mut Clock,
    projection_identity_sha256: &str,
    projection_kind: AdjustmentRevisionProjectionKind,
) -> Result<AdjustmentRevisionStageReceipt, PrivateRevisionArchiveError>
where
    Storage: PrivateRevisionArchiveStorage,
    Codec: PrivateRevisionArchiveCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    // return the first durable clock on an exact retry
    if let Some(bytes) = storage.read_stage_receipt(
        projection_identity_sha256,
        PRIVATE_REVISION_STAGE_RECEIPT_MAXIMUM_BYTES,
    )? {
        return decode_bound_stage_receipt(
            codec,
            &bytes,
            projection_identity_sha256,
            projection_kind,
            projection_identity_sha256,
        );
    }
    let durable_at = canonical_now(codec, clock)?;
    let unsigned = PrivateRevisionStageUnsigned {
        durable_at: &durable_at,
        projection_identity_sha256,
        projection_kind,
        projection_sha256: projection_identity_sha256,
    };
    let unsigned_bytes = codec
        .stage_unsigned_bytes(unsigned)
        .ok_or(PrivateRevisionArchiveError::Input)?;
    let mut receipt = AdjustmentRevisionStageReceipt {
        contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
        durable: AdjustmentRevisionStageReceiptDurableLiteral,
        durableAt: durable_at,
        projectionIdentitySha256: projection_identity_sha256.to_owned(),
        projectionKind: projection_kind,
        projectionSha256: projection_identity_sha256.to_owned(),
        stageReceiptSha256: lower_hex(codec.sha256(&unsigned_bytes)),
    };
    let receipt_bytes = codec
        .stage_receipt_bytes(&receipt)
        .ok_or(PrivateRevisionArchiveError::Input)?;
    // converge a racing writer on the original receipt clock
    if storage.write_stage_receipt_exclusive(projection_identity_sha256, &receipt_bytes)?
        == PrivateRevisionExclusiveWrite::AlreadyExists
    {
        let bytes = storage
            .read_stage_receipt(
                projection_identity_sha256,
                PRIVATE_REVISION_STAGE_RECEIPT_MAXIMUM_BYTES,
            )?
            .ok_or(PrivateRevisionArchiveError::StoredReceipt)?;
        receipt = decode_bound_stage_receipt(
            codec,
            &bytes,
            projection_identity_sha256,
            projection_kind,
            projection_identity_sha256,
        )?;
    }
    Ok(receipt)
}

/// read and bind one retained stage receipt to the requested projection.
fn read_bound_stage_receipt(
    storage: &mut impl PrivateRevisionArchiveStorage,
    codec: &impl PrivateRevisionArchiveCodec,
    projection_identity_sha256: &str,
    projection_kind: AdjustmentRevisionProjectionKind,
    projection_sha256: &str,
) -> Result<AdjustmentRevisionStageReceipt, PrivateRevisionArchiveError> {
    let bytes = storage
        .read_stage_receipt(
            projection_identity_sha256,
            PRIVATE_REVISION_STAGE_RECEIPT_MAXIMUM_BYTES,
        )?
        .ok_or(PrivateRevisionArchiveError::Binding)?;
    decode_bound_stage_receipt(
        codec,
        &bytes,
        projection_identity_sha256,
        projection_kind,
        projection_sha256,
    )
}

/// validate one canonical retained stage receipt and its unsigned identity.
fn decode_bound_stage_receipt(
    codec: &impl PrivateRevisionArchiveCodec,
    bytes: &[u8],
    projection_identity_sha256: &str,
    projection_kind: AdjustmentRevisionProjectionKind,
    projection_sha256: &str,
) -> Result<AdjustmentRevisionStageReceipt, PrivateRevisionArchiveError> {
    let receipt = codec
        .decode_stage_receipt(bytes)
        .ok_or(PrivateRevisionArchiveError::StoredReceipt)?;
    // bind every receipt field to its exact staged body
    if instant(codec, &receipt.durableAt).is_none()
        || receipt.contractVersion != AdjustmentRevisionStageReceiptContractVersionLiteral
        || receipt.durable != AdjustmentRevisionStageReceiptDurableLiteral
        || receipt.projectionIdentitySha256 != projection_identity_sha256
        || receipt.projectionKind != projection_kind
        || receipt.projectionSha256 != projection_sha256
    {
        return Err(PrivateRevisionArchiveError::StoredReceipt);
    }
    let unsigned = PrivateRevisionStageUnsigned {
        durable_at: &receipt.durableAt,
        projection_identity_sha256: &receipt.projectionIdentitySha256,
        projection_kind: receipt.projectionKind,
        projection_sha256: &receipt.projectionSha256,
    };
    let unsigned_bytes = codec
        .stage_unsigned_bytes(unsigned)
        .ok_or(PrivateRevisionArchiveError::StoredReceipt)?;
    // authenticate the exact canonical unsigned receipt fields
    if receipt.stageReceiptSha256 != lower_hex(codec.sha256(&unsigned_bytes)) {
        return Err(PrivateRevisionArchiveError::StoredReceipt);
    }
    Ok(receipt)
}

/// persist one authenticated single receipt followed by its frontier successor.
fn persist_single_commit(
    storage: &mut impl PrivateRevisionArchiveStorage,
    codec: &impl PrivateRevisionArchiveCodec,
    revision: &AdjustmentRevisionCommitReceipt,
) -> Result<(), PrivateRevisionArchiveError> {
    let receipt_bytes = codec
        .revision_commit_receipt_bytes(revision)
        .ok_or(PrivateRevisionArchiveError::Binding)?;
    storage.write_or_verify_commit_receipt(
        &revision.frontierSha256,
        &receipt_bytes,
        PRIVATE_REVISION_COMMIT_RECEIPT_MAXIMUM_BYTES,
    )?;
    let successor_bytes = codec
        .revision_successor_bytes(revision)
        .ok_or(PrivateRevisionArchiveError::Binding)?;
    storage.write_or_verify_successor(
        &revision.predecessorFrontierSha256,
        &successor_bytes,
        PRIVATE_REVISION_COMMIT_RECEIPT_MAXIMUM_BYTES,
    )?;
    Ok(())
}

/// load or create the first durable single publication receipt.
fn single_publish_receipt<Storage, Codec, Clock>(
    storage: &mut Storage,
    codec: &Codec,
    clock: &mut Clock,
    revision: &AdjustmentRevisionCommitReceipt,
    projection_kind: AdjustmentRevisionProjectionKind,
) -> Result<AdjustmentRevisionPublishReceipt, PrivateRevisionArchiveError>
where
    Storage: PrivateRevisionArchiveStorage,
    Codec: PrivateRevisionArchiveCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    // preserve the first publication clock on retry
    if let Some(bytes) = storage.read_publish_receipt(
        &revision.projectionIdentitySha256,
        PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES,
    )? {
        return decode_bound_publish_receipt(codec, &bytes, revision, projection_kind);
    }
    let committed_at = canonical_now(codec, clock)?;
    let unsigned = PrivateRevisionPublishUnsigned {
        committed_at: &committed_at,
        projection_identity_sha256: &revision.projectionIdentitySha256,
        projection_kind,
        projection_sha256: &revision.projectionSha256,
        revision_receipt_sha256: &revision.receiptSha256,
    };
    let unsigned_bytes = codec
        .publish_unsigned_bytes(unsigned)
        .ok_or(PrivateRevisionArchiveError::Binding)?;
    let publication = AdjustmentRevisionPublishReceipt {
        committed: AdjustmentRevisionPublishReceiptCommittedLiteral,
        committedAt: committed_at,
        contractVersion: AdjustmentRevisionPublishReceiptContractVersionLiteral,
        projectionIdentitySha256: revision.projectionIdentitySha256.clone(),
        projectionKind: projection_kind,
        projectionSha256: revision.projectionSha256.clone(),
        publishReceiptSha256: lower_hex(codec.sha256(&unsigned_bytes)),
        revisionReceiptSha256: revision.receiptSha256.clone(),
    };
    let bytes = codec
        .publish_receipt_bytes(&publication)
        .ok_or(PrivateRevisionArchiveError::Binding)?;
    // converge a racing writer on the original publication clock
    if storage.write_publish_receipt_exclusive(
        &revision.projectionIdentitySha256,
        &bytes,
        PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES,
    )? == PrivateRevisionExclusiveWrite::AlreadyExists
    {
        let bytes = storage
            .read_publish_receipt(
                &revision.projectionIdentitySha256,
                PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES,
            )?
            .ok_or(PrivateRevisionArchiveError::StoredReceipt)?;
        return decode_bound_publish_receipt(codec, &bytes, revision, projection_kind);
    }
    Ok(publication)
}

/// validate one retained single publication and its unsigned identity.
fn decode_bound_publish_receipt(
    codec: &impl PrivateRevisionArchiveCodec,
    bytes: &[u8],
    revision: &AdjustmentRevisionCommitReceipt,
    projection_kind: AdjustmentRevisionProjectionKind,
) -> Result<AdjustmentRevisionPublishReceipt, PrivateRevisionArchiveError> {
    let receipt = codec
        .decode_publish_receipt(bytes)
        .ok_or(PrivateRevisionArchiveError::StoredReceipt)?;
    // bind every publication field to the authenticated database receipt
    if instant(codec, &receipt.committedAt).is_none()
        || receipt.committed != AdjustmentRevisionPublishReceiptCommittedLiteral
        || receipt.contractVersion != AdjustmentRevisionPublishReceiptContractVersionLiteral
        || receipt.projectionIdentitySha256 != revision.projectionIdentitySha256
        || receipt.projectionKind != projection_kind
        || receipt.projectionSha256 != revision.projectionSha256
        || receipt.revisionReceiptSha256 != revision.receiptSha256
    {
        return Err(PrivateRevisionArchiveError::StoredReceipt);
    }
    let unsigned = PrivateRevisionPublishUnsigned {
        committed_at: &receipt.committedAt,
        projection_identity_sha256: &receipt.projectionIdentitySha256,
        projection_kind: receipt.projectionKind,
        projection_sha256: &receipt.projectionSha256,
        revision_receipt_sha256: &receipt.revisionReceiptSha256,
    };
    let unsigned_bytes = codec
        .publish_unsigned_bytes(unsigned)
        .ok_or(PrivateRevisionArchiveError::StoredReceipt)?;
    // authenticate the exact canonical unsigned publication fields
    if receipt.publishReceiptSha256 != lower_hex(codec.sha256(&unsigned_bytes)) {
        return Err(PrivateRevisionArchiveError::StoredReceipt);
    }
    Ok(receipt)
}

/// load or create one grouped publication under the existing spool lease.
fn batch_publish_receipt(
    storage: &mut impl PrivateRevisionArchiveStorage,
    codec: &impl PrivateRevisionArchiveCodec,
    clock: &mut impl RevisionArchiveCanonicalClock,
    receipts: &[AdjustmentRevisionCommitReceipt],
    projection_kind: AdjustmentRevisionBatchPublishReceiptProjectionKindEnum,
) -> Result<AdjustmentRevisionBatchPublishReceipt, PrivateRevisionArchiveError> {
    let first = receipts.first().ok_or(PrivateRevisionArchiveError::Input)?;
    // preserve the first complete grouped publication on retry
    if let Some(bytes) = storage.read_publish_receipt(
        &first.projectionIdentitySha256,
        PRIVATE_REVISION_BATCH_PUBLISH_RECEIPT_MAXIMUM_BYTES,
    )? {
        return decode_bound_batch_publish_receipt(codec, &bytes, receipts, projection_kind);
    }
    let committed_at = canonical_now(codec, clock)?;
    let revision_receipt_sha256s = receipts
        .iter()
        .map(|receipt| receipt.receiptSha256.clone())
        .collect::<Vec<_>>();
    let unsigned = PrivateRevisionBatchPublishUnsigned {
        committed_at: &committed_at,
        projection_identity_sha256: &first.projectionIdentitySha256,
        projection_kind,
        projection_sha256: &first.projectionSha256,
        revision_receipt_sha256s: &revision_receipt_sha256s,
    };
    let unsigned_bytes = codec
        .batch_publish_unsigned_bytes(unsigned)
        .ok_or(PrivateRevisionArchiveError::Binding)?;
    let publication = AdjustmentRevisionBatchPublishReceipt {
        committed: AdjustmentRevisionBatchPublishReceiptCommittedLiteral,
        committedAt: committed_at,
        contractVersion: AdjustmentRevisionBatchPublishReceiptContractVersionLiteral,
        projectionIdentitySha256: first.projectionIdentitySha256.clone(),
        projectionKind: projection_kind,
        projectionSha256: first.projectionSha256.clone(),
        publishReceiptSha256: lower_hex(codec.sha256(&unsigned_bytes)),
        revisionReceiptSha256s: revision_receipt_sha256s,
    };
    let bytes = codec
        .batch_publish_receipt_bytes(&publication)
        .ok_or(PrivateRevisionArchiveError::Binding)?;
    // converge only a racing byte-identical grouped publication
    if storage.write_publish_receipt_exclusive(
        &first.projectionIdentitySha256,
        &bytes,
        PRIVATE_REVISION_BATCH_PUBLISH_RECEIPT_MAXIMUM_BYTES,
    )? == PrivateRevisionExclusiveWrite::AlreadyExists
    {
        let bytes = storage
            .read_publish_receipt(
                &first.projectionIdentitySha256,
                PRIVATE_REVISION_BATCH_PUBLISH_RECEIPT_MAXIMUM_BYTES,
            )?
            .ok_or(PrivateRevisionArchiveError::StoredReceipt)?;
        return decode_bound_batch_publish_receipt(codec, &bytes, receipts, projection_kind);
    }
    Ok(publication)
}

/// validate one retained grouped publication and its ordered receipt root.
fn decode_bound_batch_publish_receipt(
    codec: &impl PrivateRevisionArchiveCodec,
    bytes: &[u8],
    receipts: &[AdjustmentRevisionCommitReceipt],
    projection_kind: AdjustmentRevisionBatchPublishReceiptProjectionKindEnum,
) -> Result<AdjustmentRevisionBatchPublishReceipt, PrivateRevisionArchiveError> {
    let first = receipts.first().ok_or(PrivateRevisionArchiveError::Input)?;
    let publication = codec
        .decode_batch_publish_receipt(bytes)
        .ok_or(PrivateRevisionArchiveError::StoredReceipt)?;
    let expected_receipt_sha256s = receipts
        .iter()
        .map(|receipt| receipt.receiptSha256.clone())
        .collect::<Vec<_>>();
    // bind the complete stored publication to the ordered database receipts
    if instant(codec, &publication.committedAt).is_none()
        || publication.committed != AdjustmentRevisionBatchPublishReceiptCommittedLiteral
        || publication.contractVersion
            != AdjustmentRevisionBatchPublishReceiptContractVersionLiteral
        || publication.projectionIdentitySha256 != first.projectionIdentitySha256
        || publication.projectionKind != projection_kind
        || publication.projectionSha256 != first.projectionSha256
        || publication.revisionReceiptSha256s != expected_receipt_sha256s
    {
        return Err(PrivateRevisionArchiveError::StoredReceipt);
    }
    let unsigned = PrivateRevisionBatchPublishUnsigned {
        committed_at: &publication.committedAt,
        projection_identity_sha256: &publication.projectionIdentitySha256,
        projection_kind: publication.projectionKind,
        projection_sha256: &publication.projectionSha256,
        revision_receipt_sha256s: &publication.revisionReceiptSha256s,
    };
    let unsigned_bytes = codec
        .batch_publish_unsigned_bytes(unsigned)
        .ok_or(PrivateRevisionArchiveError::StoredReceipt)?;
    // authenticate the exact canonical unsigned publication fields
    if publication.publishReceiptSha256 != lower_hex(codec.sha256(&unsigned_bytes)) {
        return Err(PrivateRevisionArchiveError::StoredReceipt);
    }
    Ok(publication)
}

/// require one canonical archive clock from the injected owner.
fn canonical_now(
    codec: &impl PrivateRevisionArchiveCodec,
    clock: &mut impl RevisionArchiveCanonicalClock,
) -> Result<String, PrivateRevisionArchiveError> {
    let value = clock
        .canonical_now()
        .ok_or(PrivateRevisionArchiveError::Clock)?;
    // reject a noncanonical clock before retaining receipt bytes
    if instant(codec, &value).is_none() {
        return Err(PrivateRevisionArchiveError::Clock);
    }
    Ok(value)
}

/// map a database receipt kind to the staged projection grammar.
fn commit_projection_kind(
    value: AdjustmentRevisionCommitReceiptProjectionKindEnum,
) -> Option<AdjustmentRevisionProjectionKind> {
    // shadow receipts use a distinct archive path and stage grammar
    match value {
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => {
            Some(AdjustmentRevisionProjectionKind::ActualBestMatch)
        }
        AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource => {
            Some(AdjustmentRevisionProjectionKind::NativeSource)
        }
        AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput => {
            Some(AdjustmentRevisionProjectionKind::RainGateInput)
        }
        AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => {
            Some(AdjustmentRevisionProjectionKind::TargetRevision)
        }
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction => None,
    }
}

/// map the two grouped projection kinds to their publication grammar.
fn batch_projection_kind(
    value: AdjustmentRevisionProjectionKind,
) -> Option<AdjustmentRevisionBatchPublishReceiptProjectionKindEnum> {
    // reject single-only projection kinds from grouped publication
    match value {
        AdjustmentRevisionProjectionKind::ActualBestMatch => {
            Some(AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::ActualBestMatch)
        }
        AdjustmentRevisionProjectionKind::TargetRevision => {
            Some(AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::TargetRevision)
        }
        AdjustmentRevisionProjectionKind::NativeSource
        | AdjustmentRevisionProjectionKind::RainGateInput => None,
    }
}

/// verify one grouped receipt chain against its exact staged body.
fn valid_batch_chain(
    receipts: &[AdjustmentRevisionCommitReceipt],
    first: &AdjustmentRevisionCommitReceipt,
    stage: &AdjustmentRevisionStageReceipt,
) -> bool {
    // require the grouped body identity before walking its receipt chain
    if first.projectionSha256 != first.projectionIdentitySha256 {
        return false;
    }
    let mut previous: Option<&AdjustmentRevisionCommitReceipt> = None;
    // preserve the supplied receipt order as global frontier order
    for receipt in receipts {
        if receipt.projectionIdentitySha256 != first.projectionIdentitySha256
            || receipt.projectionSha256 != first.projectionSha256
            || receipt.projectionKind != first.projectionKind
            || receipt.stageReceiptSha256 != stage.stageReceiptSha256
        {
            return false;
        }
        // require one direct successor ordinal and frontier link
        if let Some(predecessor) = previous
            && (receipt.predecessorFrontierSha256 != predecessor.frontierSha256
                || !is_next_ordinal(
                    &predecessor.archiveCommitOrdinal,
                    &receipt.archiveCommitOrdinal,
                ))
        {
            return false;
        }
        previous = Some(receipt);
    }
    true
}

/// compare two canonical uint64 decimal ordinals as direct successors.
fn is_next_ordinal(previous: &str, current: &str) -> bool {
    previous
        .parse::<u64>()
        .ok()
        .and_then(|ordinal| ordinal.checked_add(1))
        .is_some_and(|expected| current.parse::<u64>() == Ok(expected))
}

/// preserve only the error classes available inside the shared lease callback.
fn private_error_to_spool(error: PrivateRevisionArchiveError) -> RevisionOnlineSpoolError {
    // keep capacity distinct while closing all other callback failures
    match error {
        PrivateRevisionArchiveError::SpoolFull => RevisionOnlineSpoolError::Capacity,
        PrivateRevisionArchiveError::Input | PrivateRevisionArchiveError::Clock => {
            RevisionOnlineSpoolError::Input
        }
        PrivateRevisionArchiveError::Binding
        | PrivateRevisionArchiveError::PermanentGap
        | PrivateRevisionArchiveError::StoredReceipt => RevisionOnlineSpoolError::IdentityConflict,
        PrivateRevisionArchiveError::Storage => RevisionOnlineSpoolError::Storage,
    }
}
