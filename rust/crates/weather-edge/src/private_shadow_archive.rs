//! Immutable private shadow stage, capsule, and publication receipt ownership.

use weather_contracts::{
    AdjustmentMaintenancePublishReceipt, AdjustmentMaintenancePublishReceiptCommittedLiteral,
    AdjustmentMaintenancePublishReceiptContractVersionEnum, AdjustmentMaintenanceStageInput,
    AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceStageReceiptContractVersionEnum,
    AdjustmentMaintenanceStageReceiptDurableLiteral, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, FieldPresence,
    MaintenanceShadowPredictionMetadata,
};

use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, instant, is_sha256, lower_hex,
};
use crate::revision_online_spool::{
    RevisionOnlinePayload, RevisionOnlinePayloadAddress, RevisionOnlinePayloadKind,
    RevisionOnlineSpoolError, RevisionOnlineSpoolStorage, admit_payload,
};

const SHADOW_STAGE_KIND: RevisionOnlinePayloadKind = RevisionOnlinePayloadKind::ShadowStage;
const SHADOW_CAPSULE_KIND: RevisionOnlinePayloadKind = RevisionOnlinePayloadKind::ShadowCapsule;
const METADATA_MAXIMUM_BYTES: usize = 4_096;

/// Closed shadow archive grammar selected by comparator presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateShadowArchiveVersion {
    V1,
    V2,
}

impl PrivateShadowArchiveVersion {
    /// Resolve the stage-receipt literal for one closed grammar.
    const fn stage_contract(self) -> AdjustmentMaintenanceStageReceiptContractVersionEnum {
        match self {
            Self::V1 => {
                AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV1
            }
            Self::V2 => {
                AdjustmentMaintenanceStageReceiptContractVersionEnum::AdjustmentShadowStageReceiptV2
            }
        }
    }

    /// Resolve the publication-receipt literal for one closed grammar.
    const fn publish_contract(self) -> AdjustmentMaintenancePublishReceiptContractVersionEnum {
        match self {
            Self::V1 => AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV1,
            Self::V2 => AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV2,
        }
    }
}

/// Canonical retained stage document supplied to the admitted JSON codec.
#[derive(Debug, Clone, PartialEq)]
pub struct PrivateShadowStageDocument {
    pub input: AdjustmentMaintenanceStageInput,
    pub stage_receipt: AdjustmentMaintenanceStageReceipt,
    pub version: PrivateShadowArchiveVersion,
}

/// Canonical retained capsule supplied to the admitted JSON codec.
#[derive(Debug, Clone, PartialEq)]
pub struct PrivateShadowCapsuleDocument {
    pub prediction_committed_at: FieldPresence<String>,
    pub revision_receipt: AdjustmentRevisionCommitReceipt,
    pub stage_input: AdjustmentMaintenanceStageInput,
    pub stage_receipt: AdjustmentMaintenanceStageReceipt,
    pub version: PrivateShadowArchiveVersion,
}

/// Compact legacy v1 or fully bound comparator v2 publication input.
#[derive(Debug, Clone, PartialEq)]
pub enum PrivateShadowPublishInput {
    V1 {
        metadata: MaintenanceShadowPredictionMetadata,
        revision_receipt: AdjustmentRevisionCommitReceipt,
        source_projection_sha256: String,
        stage_receipt_sha256: String,
    },
    V2 {
        prediction_committed_at: String,
        revision_receipt: AdjustmentRevisionCommitReceipt,
        stage_input: Box<AdjustmentMaintenanceStageInput>,
        stage_receipt: AdjustmentMaintenanceStageReceipt,
    },
}

/// Ordered unsigned fields for one stage receipt identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateShadowStageReceiptIdentity {
    pub comparator_sha256: Option<String>,
    pub contract_version: AdjustmentMaintenanceStageReceiptContractVersionEnum,
    pub durable_at: String,
    pub due_key: String,
    pub prediction_body_sha256: String,
    pub registration_sha256: String,
    pub source_projection_sha256: String,
}

/// Ordered unsigned fields for one publication receipt identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateShadowPublishReceiptIdentity {
    pub committed_at: String,
    pub comparator_sha256: Option<String>,
    pub contract_version: AdjustmentMaintenancePublishReceiptContractVersionEnum,
    pub prediction_sha256: String,
}

/// Exact frontier-successor metadata retained beside one database receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateShadowRevisionSuccessor {
    pub archive_commit_ordinal: String,
    pub frontier_sha256: String,
    pub predecessor_frontier_sha256: String,
    pub receipt_sha256: String,
}

/// Canonical shadow JSON and retained receipt validation.
pub trait PrivateShadowReceiptCodec: RevisionArchiveCanonicalCodec {
    /// Validate the database-issued revision receipt and both derived identities.
    fn admit_revision_receipt(&self, receipt: &AdjustmentRevisionCommitReceipt) -> bool;

    /// Validate the database append clock between issuance and archive commitment.
    fn admit_prediction_commit_time(
        &self,
        committed_at: &str,
        issued_at: &str,
        archive_committed_at: &str,
    ) -> bool;

    /// Encode the exact newline-framed stage receipt identity preimage.
    fn stage_receipt_identity_bytes(
        &self,
        identity: &PrivateShadowStageReceiptIdentity,
    ) -> Option<Vec<u8>>;

    /// Encode the exact canonical retained stage document.
    fn encode_stage_document(&self, document: &PrivateShadowStageDocument) -> Option<Vec<u8>>;

    /// Decode one exact canonical retained stage document.
    fn decode_stage_document(&self, bytes: &[u8]) -> Option<PrivateShadowStageDocument>;

    /// Encode the exact canonical capsule document.
    fn encode_capsule_document(&self, document: &PrivateShadowCapsuleDocument) -> Option<Vec<u8>>;

    /// Decode and rebind one exact canonical capsule document.
    fn decode_capsule_document(&self, bytes: &[u8]) -> Option<PrivateShadowCapsuleDocument>;

    /// Encode the exact newline-framed publication receipt identity preimage.
    fn publish_receipt_identity_bytes(
        &self,
        identity: &PrivateShadowPublishReceiptIdentity,
    ) -> Option<Vec<u8>>;

    /// Encode one exact canonical publication receipt.
    fn encode_publish_receipt(
        &self,
        receipt: &AdjustmentMaintenancePublishReceipt,
    ) -> Option<Vec<u8>>;

    /// Decode one exact canonical publication receipt.
    fn decode_publish_receipt(&self, bytes: &[u8]) -> Option<AdjustmentMaintenancePublishReceipt>;

    /// Encode the already-validated database revision receipt.
    fn encode_revision_receipt(&self, receipt: &AdjustmentRevisionCommitReceipt)
    -> Option<Vec<u8>>;

    /// Encode the exact derived frontier-successor document.
    fn encode_revision_successor(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
    ) -> Option<Vec<u8>>;
}

/// Producer validation added only for the mutable shadow writer.
pub trait PrivateShadowArchiveCodec: PrivateShadowReceiptCodec {
    /// Run the producer's complete stage parser over typed source and prediction bytes.
    fn admit_stage_input(&self, input: &AdjustmentMaintenanceStageInput) -> bool;
}

/// Small immutable metadata directories outside the online payload census.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrivateShadowArchiveMetadataKind {
    PublishReceipt,
    RevisionCommitReceipt,
    RevisionFrontierSuccessor,
}

impl PrivateShadowArchiveMetadataKind {
    /// Resolve the exact source-owned directory component.
    pub const fn directory(self) -> &'static str {
        match self {
            Self::PublishReceipt => "shadow-revision-publish-receipts",
            Self::RevisionCommitReceipt => "revision-commit-receipts",
            Self::RevisionFrontierSuccessor => "revision-frontier-successors",
        }
    }
}

/// Outcome of one exclusive metadata publication under its immutable address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateShadowExclusiveWrite {
    Created,
    AlreadyPresent,
}

/// Native storage operations under the existing owner-private revision root.
pub trait PrivateShadowArchiveStorage: RevisionOnlineSpoolStorage {
    /// Verify custody root and create only the fixed revision archive children.
    fn ensure_revision_root(&mut self) -> Result<(), RevisionOnlineSpoolError>;

    /// Report whether the exact scheduled admission already has a permanent gap.
    fn has_permanent_shadow_gap(
        &mut self,
        due_key: &str,
        registration_sha256: &str,
    ) -> Result<bool, RevisionOnlineSpoolError>;

    /// Publish one new online payload exclusively under its content address.
    fn write_online_exclusive(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// Read one bounded online payload outside a mutation lease.
    fn read_online(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError>;

    /// Publish or verify one byte-identical online payload.
    fn write_online_immutable(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
        bytes: &[u8],
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// Remove only the exact redundant online stage, accepting prior absence.
    fn remove_online_if_present(
        &mut self,
        address: &RevisionOnlinePayloadAddress,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// Sync the fixed online payload directory after stage retirement.
    fn sync_online_directory(
        &mut self,
        kind: RevisionOnlinePayloadKind,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// Read one bounded canonical metadata file by its admitted digest address.
    fn read_metadata(
        &mut self,
        kind: PrivateShadowArchiveMetadataKind,
        digest: &str,
        maximum_bytes: usize,
    ) -> Result<Option<Vec<u8>>, RevisionOnlineSpoolError>;

    /// Publish or verify one byte-identical metadata file.
    fn write_metadata_immutable(
        &mut self,
        kind: PrivateShadowArchiveMetadataKind,
        digest: &str,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// Publish one new metadata file or report an exact-address race.
    fn write_metadata_exclusive(
        &mut self,
        kind: PrivateShadowArchiveMetadataKind,
        digest: &str,
        bytes: &[u8],
    ) -> Result<PrivateShadowExclusiveWrite, RevisionOnlineSpoolError>;
}

/// Actual shadow archive owner over admitted codec, clock, and native storage ports.
pub struct PrivateShadowArchive<Storage, Codec, Clock> {
    storage: Storage,
    codec: Codec,
    clock: Clock,
}

impl<Storage, Codec, Clock> PrivateShadowArchive<Storage, Codec, Clock>
where
    Storage: PrivateShadowArchiveStorage,
    Codec: PrivateShadowArchiveCodec,
    Clock: RevisionArchiveCanonicalClock,
{
    /// Bind the archive workflow to its existing custody and codec owners.
    pub const fn new(storage: Storage, codec: Codec, clock: Clock) -> Self {
        Self {
            storage,
            codec,
            clock,
        }
    }

    /// Release the native storage handle back to the archive process.
    pub fn into_storage(self) -> Storage {
        self.storage
    }

    /// Stage one producer-admitted source/body pair and retain its first durable receipt.
    pub fn stage_shadow_revision(
        &mut self,
        input: AdjustmentMaintenanceStageInput,
    ) -> Result<AdjustmentMaintenanceStageReceipt, RevisionOnlineSpoolError> {
        let admitted = admit_stage_input(&self.codec, input)?;
        let address = shadow_address(SHADOW_STAGE_KIND, &admitted.input.metadata.predictionSha256)?;
        self.storage.ensure_revision_root()?;
        let codec = &self.codec;
        let clock = &mut self.clock;
        self.storage.with_revision_spool_lease(
            // keep retry detection, first clock, capacity, and publication under one lease
            |storage| {
                if let Some(existing) =
                    storage.read_existing_payload(&address, SHADOW_STAGE_KIND.maximum_bytes())?
                {
                    let stored = decode_stage_document(codec, &existing)?;
                    // return the first receipt only when the complete typed input is unchanged
                    if stored.input != admitted.input {
                        return Err(RevisionOnlineSpoolError::IdentityConflict);
                    }
                    return Ok(stored.stage_receipt);
                }
                let durable_at = read_archive_time(codec, clock)?;
                let receipt = create_stage_receipt(codec, &admitted, durable_at)?;
                let document = PrivateShadowStageDocument {
                    input: admitted.input,
                    stage_receipt: receipt.clone(),
                    version: admitted.version,
                };
                let bytes = codec
                    .encode_stage_document(&document)
                    .ok_or(RevisionOnlineSpoolError::Input)?;
                let payload = RevisionOnlinePayload::new(&address, &bytes)?;
                admit_payload(storage, &payload)?;
                storage.write_online_exclusive(&address, &bytes)?;
                Ok(receipt)
            },
        )
    }

    /// Publish one compact v1 or value-bound v2 shadow capsule and acknowledgement.
    pub fn publish_shadow_revision(
        &mut self,
        input: PrivateShadowPublishInput,
    ) -> Result<AdjustmentMaintenancePublishReceipt, RevisionOnlineSpoolError> {
        let admitted = admit_publish_input(&self.codec, input)?;
        self.storage.ensure_revision_root()?;
        // preserve permanent unqualification ahead of any staged value read
        if self.storage.has_permanent_shadow_gap(
            &admitted.metadata.dueKey,
            &admitted.metadata.registrationSha256,
        )? {
            return Err(RevisionOnlineSpoolError::IdentityConflict);
        }
        let stage_address = shadow_address(SHADOW_STAGE_KIND, &admitted.metadata.predictionSha256)?;
        let stage_bytes = self
            .storage
            .read_online(&stage_address, SHADOW_STAGE_KIND.maximum_bytes())?
            .ok_or(RevisionOnlineSpoolError::Storage)?;
        let staged = decode_stage_document(&self.codec, &stage_bytes)?;
        validate_publish_binding(&admitted, &staged)?;
        let capsule = PrivateShadowCapsuleDocument {
            prediction_committed_at: admitted.prediction_committed_at,
            revision_receipt: admitted.revision_receipt,
            stage_input: staged.input,
            stage_receipt: staged.stage_receipt,
            version: admitted.version,
        };
        let capsule_bytes = self
            .codec
            .encode_capsule_document(&capsule)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        self.persist_shadow_revision_capsule(&capsule_bytes)?;
        self.publish_receipt(&capsule)
    }

    /// Persist one parser-verified capsule in database-receipt, capsule, retirement order.
    pub fn persist_shadow_revision_capsule(
        &mut self,
        bytes: &[u8],
    ) -> Result<PrivateShadowCapsuleDocument, RevisionOnlineSpoolError> {
        let capsule = self
            .codec
            .decode_capsule_document(bytes)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        validate_capsule_document(&self.codec, &capsule)?;
        let canonical = self
            .codec
            .encode_capsule_document(&capsule)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        // retain only the one canonical representation admitted by the codec
        if canonical != bytes {
            return Err(RevisionOnlineSpoolError::IdentityConflict);
        }
        let stage_address = shadow_address(
            SHADOW_STAGE_KIND,
            &capsule.stage_input.metadata.predictionSha256,
        )?;
        let capsule_address =
            shadow_address(SHADOW_CAPSULE_KIND, &capsule.revision_receipt.receiptSha256)?;
        self.storage.ensure_revision_root()?;
        let revision_bytes = self
            .codec
            .encode_revision_receipt(&capsule.revision_receipt)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        let successor_bytes = self
            .codec
            .encode_revision_successor(&capsule.revision_receipt)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        // keep both compact database metadata documents inside their source bounds
        if revision_bytes.len() > METADATA_MAXIMUM_BYTES
            || successor_bytes.len() > METADATA_MAXIMUM_BYTES
        {
            return Err(RevisionOnlineSpoolError::Input);
        }
        let payload = RevisionOnlinePayload::new(&capsule_address, bytes)?
            .replacing_shadow_stage(&stage_address)?;
        self.storage.with_revision_spool_lease(
            // retain receipt, successor, capsule, and stage retirement as one ordered mutation
            |storage| {
                admit_payload(storage, &payload)?;
                storage.write_metadata_immutable(
                    PrivateShadowArchiveMetadataKind::RevisionCommitReceipt,
                    &capsule.revision_receipt.frontierSha256,
                    &revision_bytes,
                    METADATA_MAXIMUM_BYTES,
                )?;
                storage.write_metadata_immutable(
                    PrivateShadowArchiveMetadataKind::RevisionFrontierSuccessor,
                    &capsule.revision_receipt.predecessorFrontierSha256,
                    &successor_bytes,
                    METADATA_MAXIMUM_BYTES,
                )?;
                storage.write_online_immutable(&capsule_address, bytes)?;
                storage.remove_online_if_present(&stage_address)?;
                storage.sync_online_directory(SHADOW_STAGE_KIND)?;
                Ok(())
            },
        )?;
        Ok(capsule)
    }

    /// Return the first durable publication acknowledgement or create it last.
    fn publish_receipt(
        &mut self,
        capsule: &PrivateShadowCapsuleDocument,
    ) -> Result<AdjustmentMaintenancePublishReceipt, RevisionOnlineSpoolError> {
        let prediction_sha256 = &capsule.stage_input.metadata.predictionSha256;
        // return an exact retained acknowledgement without reading the clock again
        if let Some(existing) = self.storage.read_metadata(
            PrivateShadowArchiveMetadataKind::PublishReceipt,
            prediction_sha256,
            METADATA_MAXIMUM_BYTES,
        )? {
            return decode_publish_receipt(&self.codec, capsule, &existing);
        }
        let committed_at = read_archive_time(&self.codec, &mut self.clock)?;
        let receipt = create_publish_receipt(&self.codec, capsule, committed_at)?;
        let bytes = self
            .codec
            .encode_publish_receipt(&receipt)
            .ok_or(RevisionOnlineSpoolError::Input)?;
        // keep publication acknowledgement inside the fixed small receipt bound
        if bytes.len() > METADATA_MAXIMUM_BYTES {
            return Err(RevisionOnlineSpoolError::Input);
        }
        match self.storage.write_metadata_exclusive(
            PrivateShadowArchiveMetadataKind::PublishReceipt,
            prediction_sha256,
            &bytes,
        )? {
            PrivateShadowExclusiveWrite::Created => Ok(receipt),
            PrivateShadowExclusiveWrite::AlreadyPresent => {
                let retained = self
                    .storage
                    .read_metadata(
                        PrivateShadowArchiveMetadataKind::PublishReceipt,
                        prediction_sha256,
                        METADATA_MAXIMUM_BYTES,
                    )?
                    .ok_or(RevisionOnlineSpoolError::Storage)?;
                decode_publish_receipt(&self.codec, capsule, &retained)
            }
        }
    }
}

/// Stage input plus its source-derived comparator identity.
struct AdmittedStageInput {
    comparator_sha256: Option<String>,
    input: AdjustmentMaintenanceStageInput,
    version: PrivateShadowArchiveVersion,
}

/// A validated stage identity borrowed from the original source and prediction bodies.
struct AdmittedStageIdentity {
    comparator_sha256: Option<String>,
    version: PrivateShadowArchiveVersion,
}

/// Publish fields normalized across the compact v1 and complete v2 requests.
struct AdmittedPublishInput {
    metadata: MaintenanceShadowPredictionMetadata,
    prediction_committed_at: FieldPresence<String>,
    revision_receipt: AdjustmentRevisionCommitReceipt,
    source_projection_sha256: String,
    stage_input: Option<AdjustmentMaintenanceStageInput>,
    stage_receipt: Option<AdjustmentMaintenanceStageReceipt>,
    stage_receipt_sha256: String,
    version: PrivateShadowArchiveVersion,
}

/// Admit exact producer bytes and bind metadata hashes before storage work.
fn admit_stage_input(
    codec: &impl PrivateShadowArchiveCodec,
    input: AdjustmentMaintenanceStageInput,
) -> Result<AdmittedStageInput, RevisionOnlineSpoolError> {
    let identity = admit_stage_identity(codec, &input)?;
    Ok(AdmittedStageInput {
        comparator_sha256: identity.comparator_sha256,
        input,
        version: identity.version,
    })
}

/// Validate retained and fresh stage identities without cloning the complete private bodies.
fn admit_stage_identity(
    codec: &impl PrivateShadowArchiveCodec,
    input: &AdjustmentMaintenanceStageInput,
) -> Result<AdmittedStageIdentity, RevisionOnlineSpoolError> {
    // preserve the producer's full source/body parser as an explicit typed boundary
    if !codec.admit_stage_input(input) {
        return Err(RevisionOnlineSpoolError::Input);
    }
    let body_count = usize::try_from(input.metadata.bodyByteCount)
        .map_err(|_| RevisionOnlineSpoolError::Input)?;
    let body_sha256 = lower_hex(codec.sha256(&input.body));
    let source_sha256 = lower_hex(codec.sha256(&input.sourceProjection));
    // bind compact metadata to the exact admitted value bytes
    if body_count != input.body.len()
        || body_sha256 != input.metadata.predictionBodySha256
        || source_sha256 != input.sourceProjectionSha256
    {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    let (version, comparator_sha256) = match &input.comparator {
        FieldPresence::Missing => (PrivateShadowArchiveVersion::V1, None),
        FieldPresence::Value(comparator) => (
            PrivateShadowArchiveVersion::V2,
            Some(lower_hex(codec.sha256(comparator))),
        ),
    };
    Ok(AdmittedStageIdentity {
        comparator_sha256,
        version,
    })
}

/// Normalize the two exact Node publication grammars after typed admission.
fn admit_publish_input(
    codec: &impl PrivateShadowArchiveCodec,
    input: PrivateShadowPublishInput,
) -> Result<AdmittedPublishInput, RevisionOnlineSpoolError> {
    match input {
        PrivateShadowPublishInput::V1 {
            metadata,
            revision_receipt,
            source_projection_sha256,
            stage_receipt_sha256,
        } => {
            // retain only already-admitted digests and one valid database receipt
            if !is_sha256(&source_projection_sha256)
                || !is_sha256(&stage_receipt_sha256)
                || !codec.admit_revision_receipt(&revision_receipt)
            {
                return Err(RevisionOnlineSpoolError::Input);
            }
            Ok(AdmittedPublishInput {
                metadata,
                prediction_committed_at: FieldPresence::Missing,
                revision_receipt,
                source_projection_sha256,
                stage_input: None,
                stage_receipt: None,
                stage_receipt_sha256,
                version: PrivateShadowArchiveVersion::V1,
            })
        }
        PrivateShadowPublishInput::V2 {
            prediction_committed_at,
            revision_receipt,
            stage_input,
            stage_receipt,
        } => {
            let admitted = admit_stage_input(codec, *stage_input)?;
            // require the additive comparator grammar and its database append clock
            if admitted.version != PrivateShadowArchiveVersion::V2
                || !codec.admit_revision_receipt(&revision_receipt)
                || !codec.admit_prediction_commit_time(
                    &prediction_committed_at,
                    &admitted.input.metadata.issuedAt,
                    &revision_receipt.archiveCommittedAt,
                )
            {
                return Err(RevisionOnlineSpoolError::Input);
            }
            let stage_receipt_sha256 = stage_receipt.stageReceiptSha256.clone();
            Ok(AdmittedPublishInput {
                metadata: admitted.input.metadata.clone(),
                prediction_committed_at: FieldPresence::Value(prediction_committed_at),
                revision_receipt,
                source_projection_sha256: admitted.input.sourceProjectionSha256.clone(),
                stage_input: Some(admitted.input),
                stage_receipt: Some(stage_receipt),
                stage_receipt_sha256,
                version: PrivateShadowArchiveVersion::V2,
            })
        }
    }
}

/// Create one first durable stage receipt in exact source field order.
fn create_stage_receipt(
    codec: &impl PrivateShadowArchiveCodec,
    admitted: &AdmittedStageInput,
    durable_at: String,
) -> Result<AdjustmentMaintenanceStageReceipt, RevisionOnlineSpoolError> {
    let identity = PrivateShadowStageReceiptIdentity {
        comparator_sha256: admitted.comparator_sha256.clone(),
        contract_version: admitted.version.stage_contract(),
        durable_at: durable_at.clone(),
        due_key: admitted.input.metadata.dueKey.clone(),
        prediction_body_sha256: admitted.input.metadata.predictionBodySha256.clone(),
        registration_sha256: admitted.input.metadata.registrationSha256.clone(),
        source_projection_sha256: admitted.input.sourceProjectionSha256.clone(),
    };
    let identity_bytes = codec
        .stage_receipt_identity_bytes(&identity)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    Ok(AdjustmentMaintenanceStageReceipt {
        comparatorSha256: option_presence(identity.comparator_sha256),
        contractVersion: identity.contract_version,
        durable: AdjustmentMaintenanceStageReceiptDurableLiteral,
        durableAt: identity.durable_at,
        dueKey: identity.due_key,
        predictionBodySha256: identity.prediction_body_sha256,
        registrationSha256: identity.registration_sha256,
        sourceProjectionSha256: identity.source_projection_sha256,
        stageReceiptSha256: lower_hex(codec.sha256(&identity_bytes)),
    })
}

/// Decode and rebind one retained canonical stage document.
fn decode_stage_document(
    codec: &impl PrivateShadowArchiveCodec,
    bytes: &[u8],
) -> Result<PrivateShadowStageDocument, RevisionOnlineSpoolError> {
    // reject oversized retained bytes before invoking the admitted decoder
    if bytes.len() > SHADOW_STAGE_KIND.maximum_bytes() {
        return Err(RevisionOnlineSpoolError::Input);
    }
    let document = codec
        .decode_stage_document(bytes)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    validate_stage_document(codec, &document)?;
    let canonical = codec
        .encode_stage_document(&document)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    // retain only the exact newline-framed canonical document
    if canonical != bytes {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    Ok(document)
}

/// Rebind a retained receipt to its typed input and exact identity preimage.
fn validate_stage_document(
    codec: &impl PrivateShadowArchiveCodec,
    document: &PrivateShadowStageDocument,
) -> Result<(), RevisionOnlineSpoolError> {
    validate_stage_binding(
        codec,
        &document.input,
        &document.stage_receipt,
        document.version,
    )
}

/// Bind one retained receipt to borrowed stage bytes, preserving the complete admission checks.
fn validate_stage_binding(
    codec: &impl PrivateShadowArchiveCodec,
    input: &AdjustmentMaintenanceStageInput,
    receipt: &AdjustmentMaintenanceStageReceipt,
    version: PrivateShadowArchiveVersion,
) -> Result<(), RevisionOnlineSpoolError> {
    let admitted = admit_stage_identity(codec, input)?;
    let comparator = match &receipt.comparatorSha256 {
        FieldPresence::Missing => None,
        FieldPresence::Value(value) => Some(value.clone()),
    };
    let identity = PrivateShadowStageReceiptIdentity {
        comparator_sha256: comparator,
        contract_version: receipt.contractVersion,
        durable_at: receipt.durableAt.clone(),
        due_key: receipt.dueKey.clone(),
        prediction_body_sha256: receipt.predictionBodySha256.clone(),
        registration_sha256: receipt.registrationSha256.clone(),
        source_projection_sha256: receipt.sourceProjectionSha256.clone(),
    };
    let identity_bytes = codec
        .stage_receipt_identity_bytes(&identity)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    // require every receipt field to remain bound to the exact staged values
    if version != admitted.version
        || receipt.contractVersion != admitted.version.stage_contract()
        || instant(codec, &receipt.durableAt).is_none()
        || receipt.dueKey != input.metadata.dueKey
        || receipt.predictionBodySha256 != input.metadata.predictionBodySha256
        || receipt.registrationSha256 != input.metadata.registrationSha256
        || receipt.sourceProjectionSha256 != input.sourceProjectionSha256
        || identity.comparator_sha256 != admitted.comparator_sha256
        || receipt.stageReceiptSha256 != lower_hex(codec.sha256(&identity_bytes))
    {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    Ok(())
}

/// Bind one admitted publish request to the exact retained stage and database receipt.
fn validate_publish_binding(
    published: &AdmittedPublishInput,
    staged: &PrivateShadowStageDocument,
) -> Result<(), RevisionOnlineSpoolError> {
    // preserve the complete v2 value binding while keeping v1 intentionally compact
    if published.version != staged.version
        || published.metadata != staged.input.metadata
        || published.source_projection_sha256 != staged.input.sourceProjectionSha256
        || published.stage_receipt_sha256 != staged.stage_receipt.stageReceiptSha256
        || published
            .stage_input
            .as_ref()
            .is_some_and(|input| input != &staged.input)
        || published
            .stage_receipt
            .as_ref()
            .is_some_and(|receipt| receipt != &staged.stage_receipt)
        || published.revision_receipt.projectionKind
            != AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
        || published.revision_receipt.projectionIdentitySha256
            != published.metadata.sourceReceiptSha256
        || published.revision_receipt.projectionSha256 != published.metadata.inputSha256
        || published.revision_receipt.stageReceiptSha256 != staged.stage_receipt.stageReceiptSha256
    {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    Ok(())
}

/// Revalidate the complete typed capsule before any immutable publication.
fn validate_capsule_document(
    codec: &impl PrivateShadowArchiveCodec,
    capsule: &PrivateShadowCapsuleDocument,
) -> Result<(), RevisionOnlineSpoolError> {
    // borrow the existing body and receipt instead of allocating a second stage document
    validate_stage_binding(
        codec,
        &capsule.stage_input,
        &capsule.stage_receipt,
        capsule.version,
    )?;
    // retain one shadow database receipt bound to this exact staged prediction
    if !codec.admit_revision_receipt(&capsule.revision_receipt)
        || capsule.revision_receipt.projectionKind
            != AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
        || capsule.revision_receipt.projectionIdentitySha256
            != capsule.stage_input.metadata.sourceReceiptSha256
        || capsule.revision_receipt.projectionSha256 != capsule.stage_input.metadata.inputSha256
        || capsule.revision_receipt.stageReceiptSha256 != capsule.stage_receipt.stageReceiptSha256
    {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    match (&capsule.version, &capsule.prediction_committed_at) {
        (PrivateShadowArchiveVersion::V1, FieldPresence::Missing) => Ok(()),
        (PrivateShadowArchiveVersion::V2, FieldPresence::Value(committed_at))
            if codec.admit_prediction_commit_time(
                committed_at,
                &capsule.stage_input.metadata.issuedAt,
                &capsule.revision_receipt.archiveCommittedAt,
            ) =>
        {
            Ok(())
        }
        _ => Err(RevisionOnlineSpoolError::IdentityConflict),
    }
}

/// Create the final compact publication acknowledgement after capsule durability.
fn create_publish_receipt(
    codec: &impl PrivateShadowArchiveCodec,
    capsule: &PrivateShadowCapsuleDocument,
    committed_at: String,
) -> Result<AdjustmentMaintenancePublishReceipt, RevisionOnlineSpoolError> {
    let comparator_sha256 = match &capsule.stage_receipt.comparatorSha256 {
        FieldPresence::Missing => None,
        FieldPresence::Value(value) => Some(value.clone()),
    };
    let identity = PrivateShadowPublishReceiptIdentity {
        committed_at: committed_at.clone(),
        comparator_sha256,
        contract_version: capsule.version.publish_contract(),
        prediction_sha256: capsule.stage_input.metadata.predictionSha256.clone(),
    };
    let identity_bytes = codec
        .publish_receipt_identity_bytes(&identity)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    Ok(AdjustmentMaintenancePublishReceipt {
        committed: AdjustmentMaintenancePublishReceiptCommittedLiteral,
        committedAt: committed_at,
        comparatorSha256: option_presence(identity.comparator_sha256),
        contractVersion: identity.contract_version,
        predictionSha256: identity.prediction_sha256,
        publishReceiptSha256: lower_hex(codec.sha256(&identity_bytes)),
    })
}

/// Decode and bind one retained publication acknowledgement.
fn decode_publish_receipt(
    codec: &impl PrivateShadowArchiveCodec,
    capsule: &PrivateShadowCapsuleDocument,
    bytes: &[u8],
) -> Result<AdjustmentMaintenancePublishReceipt, RevisionOnlineSpoolError> {
    // keep the fixed metadata ceiling ahead of external decoding
    if bytes.len() > METADATA_MAXIMUM_BYTES {
        return Err(RevisionOnlineSpoolError::Input);
    }
    let receipt = codec
        .decode_publish_receipt(bytes)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    let comparator_sha256 = match &receipt.comparatorSha256 {
        FieldPresence::Missing => None,
        FieldPresence::Value(value) => Some(value.clone()),
    };
    let identity = PrivateShadowPublishReceiptIdentity {
        committed_at: receipt.committedAt.clone(),
        comparator_sha256,
        contract_version: receipt.contractVersion,
        prediction_sha256: receipt.predictionSha256.clone(),
    };
    let identity_bytes = codec
        .publish_receipt_identity_bytes(&identity)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    let canonical = codec
        .encode_publish_receipt(&receipt)
        .ok_or(RevisionOnlineSpoolError::Input)?;
    // bind the retained acknowledgement to the exact capsule and canonical bytes
    if canonical != bytes
        || instant(codec, &receipt.committedAt).is_none()
        || receipt.contractVersion != capsule.version.publish_contract()
        || receipt.predictionSha256 != capsule.stage_input.metadata.predictionSha256
        || identity.comparator_sha256
            != match &capsule.stage_receipt.comparatorSha256 {
                FieldPresence::Missing => None,
                FieldPresence::Value(value) => Some(value.clone()),
            }
        || receipt.publishReceiptSha256 != lower_hex(codec.sha256(&identity_bytes))
    {
        return Err(RevisionOnlineSpoolError::IdentityConflict);
    }
    Ok(receipt)
}

/// Read one canonical archive instant through the admitted time owner.
fn read_archive_time(
    codec: &impl RevisionArchiveCanonicalCodec,
    clock: &mut impl RevisionArchiveCanonicalClock,
) -> Result<String, RevisionOnlineSpoolError> {
    let value = clock
        .canonical_now()
        .ok_or(RevisionOnlineSpoolError::Input)?;
    // reject a noncanonical clock before embedding it in any receipt
    if instant(codec, &value).is_none() {
        return Err(RevisionOnlineSpoolError::Input);
    }
    Ok(value)
}

/// Convert one optional source field into its generated presence representation.
fn option_presence<T>(value: Option<T>) -> FieldPresence<T> {
    match value {
        Some(value) => FieldPresence::Value(value),
        None => FieldPresence::Missing,
    }
}

/// Build one fixed online address from an already-admitted digest.
fn shadow_address(
    kind: RevisionOnlinePayloadKind,
    digest: &str,
) -> Result<RevisionOnlinePayloadAddress, RevisionOnlineSpoolError> {
    RevisionOnlinePayloadAddress::from_sha256(kind, digest).ok_or(RevisionOnlineSpoolError::Input)
}
