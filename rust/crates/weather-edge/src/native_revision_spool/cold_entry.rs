//! Read one complete retained revision group into its canonical cold-page entry.

use weather_contracts::{
    AdjustmentMaintenancePublishReceipt, AdjustmentMaintenanceStageReceipt,
    AdjustmentRevisionBatchPublishReceipt, AdjustmentRevisionCommitReceipt,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionProjectionKind,
    AdjustmentRevisionPublishReceipt, AdjustmentRevisionStageReceipt, FieldPresence,
    MaintenanceShadowGapReason, RainMaintenanceControlStateStageReceipt,
};

use super::{
    NativeRevisionColdGroup, NativeRevisionSpool, RevisionSpoolLeaseCodec, list_names,
    read_optional_file, read_private_file, spool_error,
};
use crate::permanent_archive_gap::{PermanentArchiveGapInput, bounded_gap_bytes};
use crate::private_archive_service::{
    AdjustmentMaintenanceGapReceipt, AdjustmentRevisionGapReceipt,
};
use crate::private_revision_archive::{
    AdmittedPrivateRevisionProjection, PRIVATE_REVISION_BATCH_PUBLISH_RECEIPT_MAXIMUM_BYTES,
    PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES, PRIVATE_REVISION_STAGE_RECEIPT_MAXIMUM_BYTES,
    PrivateRevisionBatchPublishUnsigned, PrivateRevisionProjectionDependency,
    PrivateRevisionProjectionShape, PrivateRevisionPublishUnsigned, PrivateRevisionReceiptCodec,
    PrivateRevisionStageUnsigned,
};
use crate::private_shadow_archive::{
    PrivateShadowArchiveVersion, PrivateShadowCapsuleDocument, PrivateShadowPublishReceiptIdentity,
    PrivateShadowReceiptCodec,
};
use crate::rain_control_state_archive::decode_canonical_rain_stage_receipt;
use crate::revision_archive_canonical::{instant, is_payload_name, lower_hex};
use crate::revision_gap_transfer::RevisionGapSha256;
use crate::revision_online_spool::{RevisionOnlinePayloadKind, RevisionOnlineSpoolError};

const REVISION_STAGES: &str = "revision-stage-receipts";
const REVISION_PUBLICATIONS: &str = "revision-publish-receipts";
const REVISION_GAPS: &str = "revision-gaps";
const SHADOW_PUBLICATIONS: &str = "shadow-revision-publish-receipts";
const SHADOW_GAPS: &str = "shadow-revision-gaps";
const RAIN_STAGES: &str = "rain-control-state-stage-receipts";
const MAXIMUM_GAPS: usize = 4_096;
const METADATA_BYTES: usize = 4_096;

/// Closed payload contracts that may be carried by the original cold-page wire entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRevisionColdPayloadKind {
    Revision,
    BatchRevision,
    RainGateFeature,
    RainGateControl,
    RainFixedGaugeTarget,
    TemperatureNativeSource,
    ShadowV1,
    ShadowV2,
}

/// One validated successful publication or its one retained terminal disposition.
#[derive(Debug, Clone, PartialEq)]
pub enum NativeRevisionColdPublication {
    Revision(AdjustmentRevisionPublishReceipt),
    Batch(AdjustmentRevisionBatchPublishReceipt),
    Shadow(AdjustmentMaintenancePublishReceipt),
    RevisionGap(AdjustmentRevisionGapReceipt),
    ShadowGap(AdjustmentMaintenanceGapReceipt),
}

/// The first durable receipt grammar selected by the payload family.
#[derive(Debug, Clone, PartialEq)]
pub enum NativeRevisionColdStageReceipt {
    Revision(AdjustmentRevisionStageReceipt),
    Shadow(AdjustmentMaintenanceStageReceipt),
}

/// Exact additional monthly state carried beside one rain-control projection.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeRevisionColdRainControl {
    pub state_bytes: Vec<u8>,
    pub state_sha256: String,
    pub stage_receipt: RainMaintenanceControlStateStageReceipt,
}

/// Fully admitted source material supplied to the canonical cold-entry encoder.
pub struct NativeRevisionColdEntryMaterial<'a> {
    pub batch: bool,
    pub payload_bytes: &'a [u8],
    pub payload_identity_sha256: String,
    pub payload_kind: NativeRevisionColdPayloadKind,
    pub publication: NativeRevisionColdPublication,
    pub rain_control: Option<NativeRevisionColdRainControl>,
    pub receipts: &'a [AdjustmentRevisionCommitReceipt],
    pub stage_receipt: NativeRevisionColdStageReceipt,
    pub successor_bytes: Vec<&'a [u8]>,
}

/// Existing JSON, archive, base64, and identity effects needed by the cold reader.
pub trait NativeRevisionColdEntryCodec:
    PrivateRevisionReceiptCodec + PrivateShadowReceiptCodec + super::NativeRevisionSpoolArtifactCodec
{
    /// Decode one canonical database receipt using the complete original grammar.
    fn admitted_cold_commit_receipt(&self, bytes: &[u8])
    -> Option<AdjustmentRevisionCommitReceipt>;

    /// Read only the original cold-page projection metadata from already retained bytes.
    /// This is not admission for a new producer write.
    fn admitted_cold_projection(
        &self,
        bytes: &[u8],
    ) -> Option<(
        NativeRevisionColdPayloadKind,
        AdmittedPrivateRevisionProjection,
    )>;

    /// Encode one fully validated source material set as the exact canonical entry plus line feed.
    fn encode_cold_entry(&self, material: &NativeRevisionColdEntryMaterial<'_>) -> Option<Vec<u8>>;
}

/// Closed failures produced before any cold page can use the retained group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeRevisionColdEntryError {
    Input,
    Storage,
    Census,
    IdentityConflict,
}

impl From<RevisionOnlineSpoolError> for NativeRevisionColdEntryError {
    /// Preserve the existing archive error classes without exposing capacity as a read result.
    fn from(error: RevisionOnlineSpoolError) -> Self {
        // preserve error class
        match error {
            RevisionOnlineSpoolError::Input | RevisionOnlineSpoolError::Capacity => Self::Input,
            RevisionOnlineSpoolError::Storage => Self::Storage,
            RevisionOnlineSpoolError::Census => Self::Census,
            RevisionOnlineSpoolError::IdentityConflict => Self::IdentityConflict,
        }
    }
}

impl<Lease: RevisionSpoolLeaseCodec> NativeRevisionSpool<Lease> {
    /// Read and bind one direct single or grouped successor as a canonical cold-page entry.
    pub fn read_revision_cold_entry(
        &self,
        group: &NativeRevisionColdGroup,
        codec: &impl NativeRevisionColdEntryCodec,
    ) -> Result<Vec<u8>, NativeRevisionColdEntryError> {
        let receipts = admit_group_receipts(group, codec)?;
        let first = receipts
            .first()
            .ok_or(NativeRevisionColdEntryError::Input)?;
        let (payload_bytes, payload_kind, admitted_projection, shadow_capsule) =
            self.read_payload(first, codec)?;
        admit_group_shape(group.batch, &receipts, admitted_projection.as_ref())?;
        let stage_receipt = self.read_stage_receipt(first, shadow_capsule.as_ref(), codec)?;
        let publication = self.read_publication(
            group.batch,
            first,
            &receipts,
            shadow_capsule.as_ref(),
            codec,
        )?;
        let rain_control = self.read_rain_control(admitted_projection.as_ref(), codec)?;
        let successor_bytes = group
            .successors
            .iter()
            // borrow canonical members
            .map(|successor| successor.canonical_bytes.as_slice())
            .collect::<Vec<_>>();
        let material = NativeRevisionColdEntryMaterial {
            batch: group.batch,
            payload_identity_sha256: lower_hex(codec.sha256(&payload_bytes)),
            payload_bytes: &payload_bytes,
            payload_kind,
            publication,
            rain_control,
            receipts: &receipts,
            stage_receipt,
            successor_bytes,
        };
        let bytes = codec
            .encode_cold_entry(&material)
            .ok_or(NativeRevisionColdEntryError::Input)?;
        // retain one canonical JSON record while leaving aggregate size deferral to the page owner.
        if bytes.len() < 3
            || bytes.last() != Some(&b'\n')
            || bytes[..bytes.len() - 1].contains(&b'\n')
        {
            return Err(NativeRevisionColdEntryError::IdentityConflict);
        }
        Ok(bytes)
    }

    /// Read one projection or shadow capsule and bind it to the database receipt.
    fn read_payload(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
        codec: &impl NativeRevisionColdEntryCodec,
    ) -> Result<PayloadRead, NativeRevisionColdEntryError> {
        // select the shadow capsule only for the original shadow database kind.
        if receipt.projectionKind
            == AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
        {
            let bytes = self.read_required_digest(
                RevisionOnlinePayloadKind::ShadowCapsule.directory(),
                &receipt.receiptSha256,
                RevisionOnlinePayloadKind::ShadowCapsule.maximum_bytes(),
            )?;
            let capsule = codec
                .decode_capsule_document(&bytes)
                .ok_or(NativeRevisionColdEntryError::Input)?;
            let canonical = codec
                .encode_capsule_document(&capsule)
                .ok_or(NativeRevisionColdEntryError::Input)?;
            // require the retained capsule to carry the exact authenticated database receipt.
            if canonical != bytes || capsule.revision_receipt != *receipt {
                return Err(NativeRevisionColdEntryError::IdentityConflict);
            }
            // retain capsule version
            let kind = match capsule.version {
                PrivateShadowArchiveVersion::V1 => NativeRevisionColdPayloadKind::ShadowV1,
                PrivateShadowArchiveVersion::V2 => NativeRevisionColdPayloadKind::ShadowV2,
            };
            return Ok((bytes, kind, None, Some(capsule)));
        }

        let bytes = self.read_required_digest(
            RevisionOnlinePayloadKind::Projection.directory(),
            &receipt.projectionIdentitySha256,
            RevisionOnlinePayloadKind::Projection.maximum_bytes(),
        )?;
        let (kind, projection) = codec
            .admitted_cold_projection(&bytes)
            .ok_or(NativeRevisionColdEntryError::Input)?;
        // bind both receipt body hashes and the closed producer projection family.
        if receipt.projectionSha256 != receipt.projectionIdentitySha256
            || lower_hex(codec.sha256(&bytes)) != receipt.projectionIdentitySha256
            || !projection_kind_matches(receipt.projectionKind, projection.projection_kind)
        {
            return Err(NativeRevisionColdEntryError::IdentityConflict);
        }
        Ok((bytes, kind, Some(projection), None))
    }

    /// Validate the first durable stage metadata selected by the payload grammar.
    fn read_stage_receipt(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
        shadow: Option<&PrivateShadowCapsuleDocument>,
        codec: &impl NativeRevisionColdEntryCodec,
    ) -> Result<NativeRevisionColdStageReceipt, NativeRevisionColdEntryError> {
        // the complete capsule already contains the shadow stage receipt.
        if let Some(capsule) = shadow {
            return Ok(NativeRevisionColdStageReceipt::Shadow(
                capsule.stage_receipt.clone(),
            ));
        }
        let bytes = self.read_required_digest(
            REVISION_STAGES,
            &receipt.projectionIdentitySha256,
            PRIVATE_REVISION_STAGE_RECEIPT_MAXIMUM_BYTES,
        )?;
        let value = PrivateRevisionReceiptCodec::decode_stage_receipt(codec, &bytes)
            .ok_or(NativeRevisionColdEntryError::Input)?;
        let projection_kind = projection_kind(receipt.projectionKind)
            .ok_or(NativeRevisionColdEntryError::IdentityConflict)?;
        let unsigned = PrivateRevisionStageUnsigned {
            durable_at: &value.durableAt,
            projection_identity_sha256: &value.projectionIdentitySha256,
            projection_kind: value.projectionKind,
            projection_sha256: &value.projectionSha256,
        };
        let unsigned_bytes = codec
            .stage_unsigned_bytes(unsigned)
            .ok_or(NativeRevisionColdEntryError::Input)?;
        let canonical = codec
            .stage_receipt_bytes(&value)
            .ok_or(NativeRevisionColdEntryError::Input)?;
        // authenticate the exact stage receipt and its complete projection binding.
        if canonical != bytes
            || instant(codec, &value.durableAt).is_none()
            || value.projectionIdentitySha256 != receipt.projectionIdentitySha256
            || value.projectionKind != projection_kind
            || value.projectionSha256 != receipt.projectionSha256
            || value.stageReceiptSha256 != receipt.stageReceiptSha256
            || value.stageReceiptSha256 != lower_hex(codec.sha256(&unsigned_bytes))
        {
            return Err(NativeRevisionColdEntryError::IdentityConflict);
        }
        Ok(NativeRevisionColdStageReceipt::Revision(value))
    }

    /// Read either the exact successful publication or one exact permanent terminal gap.
    fn read_publication(
        &self,
        batch: bool,
        first: &AdjustmentRevisionCommitReceipt,
        receipts: &[AdjustmentRevisionCommitReceipt],
        shadow: Option<&PrivateShadowCapsuleDocument>,
        codec: &impl NativeRevisionColdEntryCodec,
    ) -> Result<NativeRevisionColdPublication, NativeRevisionColdEntryError> {
        // preserve the separate shadow publication grammar and address.
        if let Some(capsule) = shadow {
            return self.read_shadow_publication(capsule, codec);
        }
        // select receipt bound
        let maximum = if batch {
            PRIVATE_REVISION_BATCH_PUBLISH_RECEIPT_MAXIMUM_BYTES
        } else {
            PRIVATE_REVISION_PUBLISH_RECEIPT_MAXIMUM_BYTES
        };
        let bytes = self.read_optional_digest(
            REVISION_PUBLICATIONS,
            &first.projectionIdentitySha256,
            maximum,
        )?;
        // admit a present publication before consulting terminal gaps.
        if let Some(bytes) = bytes {
            // select publication grammar
            return if batch {
                admit_batch_publication(&bytes, receipts, codec)
                    .map(NativeRevisionColdPublication::Batch)
            } else {
                admit_single_publication(&bytes, first, codec)
                    .map(NativeRevisionColdPublication::Revision)
            };
        }
        self.read_revision_gap(first, codec)
            .map(NativeRevisionColdPublication::RevisionGap)
    }

    /// Read one shadow publication receipt or the matching scheduled terminal gap.
    fn read_shadow_publication(
        &self,
        capsule: &PrivateShadowCapsuleDocument,
        codec: &impl NativeRevisionColdEntryCodec,
    ) -> Result<NativeRevisionColdPublication, NativeRevisionColdEntryError> {
        let prediction = &capsule.stage_input.metadata.predictionSha256;
        let bytes = self.read_optional_digest(SHADOW_PUBLICATIONS, prediction, METADATA_BYTES)?;
        // bind a present receipt to the complete retained capsule.
        if let Some(bytes) = bytes {
            let value = PrivateShadowReceiptCodec::decode_publish_receipt(codec, &bytes)
                .ok_or(NativeRevisionColdEntryError::Input)?;
            // normalize optional comparator
            let comparator_sha256 = match &value.comparatorSha256 {
                FieldPresence::Missing => None,
                FieldPresence::Value(value) => Some(value.clone()),
            };
            let identity = PrivateShadowPublishReceiptIdentity {
                committed_at: value.committedAt.clone(),
                comparator_sha256,
                contract_version: value.contractVersion,
                prediction_sha256: value.predictionSha256.clone(),
            };
            let identity_bytes = codec
                .publish_receipt_identity_bytes(&identity)
                .ok_or(NativeRevisionColdEntryError::Input)?;
            let canonical = codec
                .encode_publish_receipt(&value)
                .ok_or(NativeRevisionColdEntryError::Input)?;
            // normalize capsule comparator
            let expected_comparator = match &capsule.stage_receipt.comparatorSha256 {
                FieldPresence::Missing => None,
                FieldPresence::Value(value) => Some(value.clone()),
            };
            // preserve every source binding from the capsule publication parser.
            if canonical != bytes
                || instant(codec, &value.committedAt).is_none()
                || !shadow_publish_version_matches(capsule.version, value.contractVersion)
                || value.predictionSha256 != *prediction
                || identity.comparator_sha256 != expected_comparator
                || value.publishReceiptSha256 != lower_hex(codec.sha256(&identity_bytes))
            {
                return Err(NativeRevisionColdEntryError::IdentityConflict);
            }
            return Ok(NativeRevisionColdPublication::Shadow(value));
        }
        self.read_shadow_gap(capsule, codec)
            .map(NativeRevisionColdPublication::ShadowGap)
    }

    /// Read and cross-bind the optional rain control state named by the projection.
    fn read_rain_control(
        &self,
        projection: Option<&AdmittedPrivateRevisionProjection>,
        codec: &impl NativeRevisionColdEntryCodec,
    ) -> Result<Option<NativeRevisionColdRainControl>, NativeRevisionColdEntryError> {
        let Some(AdmittedPrivateRevisionProjection {
            dependency:
                PrivateRevisionProjectionDependency::RainControlState {
                    ordinal_artifact_sha256,
                    state_sha256,
                    state_stage_receipt_sha256,
                },
            ..
        }) = projection
        else {
            return Ok(None);
        };
        let state_bytes = self.read_required_digest(
            RevisionOnlinePayloadKind::RainControlState.directory(),
            state_sha256,
            RevisionOnlinePayloadKind::RainControlState.maximum_bytes(),
        )?;
        let state = codec
            .decode_control_json(&state_bytes)
            .ok_or(NativeRevisionColdEntryError::Input)?;
        let canonical = codec
            .encode_control_sorted_json(&state)
            .ok_or(NativeRevisionColdEntryError::Input)?;
        let stage_bytes = self.read_required_digest(RAIN_STAGES, state_sha256, METADATA_BYTES)?;
        let stage_receipt = decode_canonical_rain_stage_receipt(codec, &stage_bytes, state_sha256)?;
        // bind state bytes, its ordinal artifact, and first durable stage identity.
        if canonical != state_bytes
            || lower_hex(codec.sha256(&state_bytes)) != *state_sha256
            || codec
                .admitted_rain_control_ordinal_sha256(&state_bytes)
                .as_deref()
                != Some(ordinal_artifact_sha256.as_str())
            || stage_receipt.stageReceiptSha256 != *state_stage_receipt_sha256
        {
            return Err(NativeRevisionColdEntryError::IdentityConflict);
        }
        Ok(Some(NativeRevisionColdRainControl {
            state_bytes,
            state_sha256: state_sha256.clone(),
            stage_receipt,
        }))
    }

    /// Read one required lower-hex addressed private file inside a fixed directory.
    fn read_required_digest(
        &self,
        directory: &str,
        digest: &str,
        maximum: usize,
    ) -> Result<Vec<u8>, NativeRevisionColdEntryError> {
        self.read_optional_digest(directory, digest, maximum)?
            .ok_or(NativeRevisionColdEntryError::Storage)
    }

    /// Read one optional lower-hex addressed private file inside a fixed directory.
    fn read_optional_digest(
        &self,
        directory: &str,
        digest: &str,
        maximum: usize,
    ) -> Result<Option<Vec<u8>>, NativeRevisionColdEntryError> {
        let name = Self::metadata_name(digest).map_err(spool_error)?;
        read_optional_file(&self.root.join(directory).join(name), maximum)
            .map_err(|_| NativeRevisionColdEntryError::Storage)
    }

    /// Scan every revision gap and select exactly one terminal publication disposition.
    fn read_revision_gap(
        &self,
        receipt: &AdjustmentRevisionCommitReceipt,
        codec: &impl NativeRevisionColdEntryCodec,
    ) -> Result<AdjustmentRevisionGapReceipt, NativeRevisionColdEntryError> {
        let mut selected = None;
        // validate the complete bounded gap census before selecting a matching value.
        for (name, bytes) in self.read_gap_files(REVISION_GAPS)? {
            let gap = codec
                .accepted_revision_gap(&bytes)
                .ok_or(NativeRevisionColdEntryError::Census)?;
            let input = PermanentArchiveGapInput::Revision(&gap.input);
            input.validated_kind()?;
            let key = bounded_gap_bytes(codec.encode_gap_key(input))?;
            // require the immutable filename and exact terminal projection match.
            if name != format!("sha256-{}.json", lower_hex(codec.sha256(&key))) {
                return Err(NativeRevisionColdEntryError::IdentityConflict);
            }
            // normalize optional projection
            let projection = match &gap.input.projectionIdentitySha256 {
                weather_contracts::Nullable::Null => None,
                weather_contracts::Nullable::Value(value) => Some(value.as_str()),
            };
            // select exact terminal gap
            if projection == Some(receipt.projectionIdentitySha256.as_str())
                && gap.input.projectionKind
                    == projection_kind(receipt.projectionKind)
                        .ok_or(NativeRevisionColdEntryError::IdentityConflict)?
                && gap.input.reason
                    == weather_contracts::AdjustmentRevisionGapReason::ArchivePublishFailed
            {
                // reject multiple supposedly permanent outcomes for one projection.
                if selected.replace(gap).is_some() {
                    return Err(NativeRevisionColdEntryError::IdentityConflict);
                }
            }
        }
        let gap = selected.ok_or(NativeRevisionColdEntryError::Storage)?;
        // the unique terminal disposition must also bind the exact archived body
        if gap.input.projectionSha256
            != weather_contracts::Nullable::Value(receipt.projectionSha256.clone())
        {
            return Err(NativeRevisionColdEntryError::IdentityConflict);
        }
        Ok(gap)
    }

    /// Scan every shadow gap and select exactly one terminal scheduled disposition.
    fn read_shadow_gap(
        &self,
        capsule: &PrivateShadowCapsuleDocument,
        codec: &impl NativeRevisionColdEntryCodec,
    ) -> Result<AdjustmentMaintenanceGapReceipt, NativeRevisionColdEntryError> {
        let mut selected = None;
        // validate the complete bounded gap census before selecting a matching value.
        for (name, bytes) in self.read_gap_files(SHADOW_GAPS)? {
            let gap = codec
                .accepted_shadow_gap(&bytes)
                .ok_or(NativeRevisionColdEntryError::Census)?;
            let input = PermanentArchiveGapInput::Shadow(&gap.input);
            input.validated_kind()?;
            let key = bounded_gap_bytes(codec.encode_gap_key(input))?;
            // require the immutable filename and exact scheduled terminal match.
            if name != format!("sha256-{}.json", lower_hex(codec.sha256(&key))) {
                return Err(NativeRevisionColdEntryError::IdentityConflict);
            }
            // select exact terminal gap
            if gap.input.dueKey == capsule.stage_input.metadata.dueKey
                && gap.input.registrationSha256 == capsule.stage_input.metadata.registrationSha256
                && gap.input.reason == MaintenanceShadowGapReason::ArchivePublishFailed
            {
                // reject multiple supposedly permanent outcomes for one scheduled capture.
                if selected.replace(gap).is_some() {
                    return Err(NativeRevisionColdEntryError::IdentityConflict);
                }
            }
        }
        selected.ok_or(NativeRevisionColdEntryError::Storage)
    }

    /// Return a complete bounded set of private permanent-gap bytes.
    fn read_gap_files(
        &self,
        directory: &str,
    ) -> Result<Vec<(String, Vec<u8>)>, NativeRevisionColdEntryError> {
        let names = list_names(&self.root.join(directory), Some(MAXIMUM_GAPS + 1))
            .map_err(|_| NativeRevisionColdEntryError::Storage)?;
        // reject an incomplete or noncanonical permanent metadata census.
        if names.len() > MAXIMUM_GAPS || !names.iter().map(String::as_str).all(is_payload_name) {
            return Err(NativeRevisionColdEntryError::Census);
        }
        let mut files = Vec::with_capacity(names.len());
        // retain the directory's complete admitted set for exact-one selection.
        for name in names {
            let bytes = read_private_file(&self.root.join(directory).join(&name), METADATA_BYTES)
                .map_err(|_| NativeRevisionColdEntryError::Storage)?;
            files.push((name, bytes));
        }
        Ok(files)
    }
}

type PayloadRead = (
    Vec<u8>,
    NativeRevisionColdPayloadKind,
    Option<AdmittedPrivateRevisionProjection>,
    Option<PrivateShadowCapsuleDocument>,
);

/// Decode every complete receipt and cross-check the reduced group projection.
fn admit_group_receipts(
    group: &NativeRevisionColdGroup,
    codec: &impl NativeRevisionColdEntryCodec,
) -> Result<Vec<AdjustmentRevisionCommitReceipt>, NativeRevisionColdEntryError> {
    let mut values = Vec::with_capacity(group.receipts.len());
    // do not trust reduced fields independently of their canonical receipt bytes.
    for reduced in &group.receipts {
        let value = codec
            .admitted_cold_commit_receipt(&reduced.canonical_bytes)
            .ok_or(NativeRevisionColdEntryError::Input)?;
        let canonical = codec
            .revision_commit_receipt_bytes(&value)
            .ok_or(NativeRevisionColdEntryError::Input)?;
        // require all reduced cursor fields to match the complete canonical receipt.
        if canonical != reduced.canonical_bytes
            || value.archiveCommitOrdinal.parse::<u64>().ok()
                != Some(reduced.archive_commit_ordinal)
            || RevisionGapSha256::from_lower_hex(&value.frontierSha256)
                != Some(reduced.frontier_sha256)
            || RevisionGapSha256::from_lower_hex(&value.predecessorFrontierSha256)
                != Some(reduced.predecessor_frontier_sha256)
            || RevisionGapSha256::from_lower_hex(&value.projectionIdentitySha256)
                != Some(reduced.projection_identity_sha256)
            || RevisionGapSha256::from_lower_hex(&value.receiptSha256)
                != Some(reduced.receipt_sha256)
            || !codec.validate_revision_commit_receipt(&value)
            || !codec.validate_revision_commit_receipt_identity(&value)
        {
            return Err(NativeRevisionColdEntryError::IdentityConflict);
        }
        values.push(value);
    }
    Ok(values)
}

/// Require grouped payload row count and single/group receipt cardinality to agree.
fn admit_group_shape(
    batch: bool,
    receipts: &[AdjustmentRevisionCommitReceipt],
    projection: Option<&AdmittedPrivateRevisionProjection>,
) -> Result<(), NativeRevisionColdEntryError> {
    // compare representation shape
    let valid = match (batch, projection.map(|value| value.shape)) {
        (false, None) => receipts.len() == 1,
        (false, Some(PrivateRevisionProjectionShape::Single)) => receipts.len() == 1,
        (true, Some(PrivateRevisionProjectionShape::Grouped { row_count })) => {
            receipts.len() == row_count && row_count > 0
        }
        _ => false,
    };
    // reject a group representation that would split or relabel one projection body.
    if !valid {
        return Err(NativeRevisionColdEntryError::IdentityConflict);
    }
    Ok(())
}

/// Admit one canonical single publication against its authenticated database receipt.
fn admit_single_publication(
    bytes: &[u8],
    revision: &AdjustmentRevisionCommitReceipt,
    codec: &impl NativeRevisionColdEntryCodec,
) -> Result<AdjustmentRevisionPublishReceipt, NativeRevisionColdEntryError> {
    let value = PrivateRevisionReceiptCodec::decode_publish_receipt(codec, bytes)
        .ok_or(NativeRevisionColdEntryError::Input)?;
    let projection_kind = projection_kind(revision.projectionKind)
        .ok_or(NativeRevisionColdEntryError::IdentityConflict)?;
    let unsigned = PrivateRevisionPublishUnsigned {
        committed_at: &value.committedAt,
        projection_identity_sha256: &value.projectionIdentitySha256,
        projection_kind: value.projectionKind,
        projection_sha256: &value.projectionSha256,
        revision_receipt_sha256: &value.revisionReceiptSha256,
    };
    let unsigned_bytes = codec
        .publish_unsigned_bytes(unsigned)
        .ok_or(NativeRevisionColdEntryError::Input)?;
    let canonical = codec
        .publish_receipt_bytes(&value)
        .ok_or(NativeRevisionColdEntryError::Input)?;
    // bind all publication fields and canonical bytes to the database receipt.
    if canonical != bytes
        || instant(codec, &value.committedAt).is_none()
        || value.projectionIdentitySha256 != revision.projectionIdentitySha256
        || value.projectionKind != projection_kind
        || value.projectionSha256 != revision.projectionSha256
        || value.revisionReceiptSha256 != revision.receiptSha256
        || value.publishReceiptSha256 != lower_hex(codec.sha256(&unsigned_bytes))
    {
        return Err(NativeRevisionColdEntryError::IdentityConflict);
    }
    Ok(value)
}

/// Admit one canonical grouped publication against every ordered database receipt.
fn admit_batch_publication(
    bytes: &[u8],
    revisions: &[AdjustmentRevisionCommitReceipt],
    codec: &impl NativeRevisionColdEntryCodec,
) -> Result<AdjustmentRevisionBatchPublishReceipt, NativeRevisionColdEntryError> {
    let first = revisions
        .first()
        .ok_or(NativeRevisionColdEntryError::Input)?;
    let value = codec
        .decode_batch_publish_receipt(bytes)
        .ok_or(NativeRevisionColdEntryError::Input)?;
    let expected_receipts = revisions
        .iter()
        .map(|receipt| receipt.receiptSha256.clone())
        .collect::<Vec<_>>();
    let projection_kind = batch_projection_kind(first.projectionKind)
        .ok_or(NativeRevisionColdEntryError::IdentityConflict)?;
    let unsigned = PrivateRevisionBatchPublishUnsigned {
        committed_at: &value.committedAt,
        projection_identity_sha256: &value.projectionIdentitySha256,
        projection_kind: value.projectionKind,
        projection_sha256: &value.projectionSha256,
        revision_receipt_sha256s: &value.revisionReceiptSha256s,
    };
    let unsigned_bytes = codec
        .batch_publish_unsigned_bytes(unsigned)
        .ok_or(NativeRevisionColdEntryError::Input)?;
    let canonical = codec
        .batch_publish_receipt_bytes(&value)
        .ok_or(NativeRevisionColdEntryError::Input)?;
    // bind the publication to the shared body and complete ordered receipt set.
    if canonical != bytes
        || instant(codec, &value.committedAt).is_none()
        || value.projectionIdentitySha256 != first.projectionIdentitySha256
        || value.projectionKind != projection_kind
        || value.projectionSha256 != first.projectionSha256
        || value.revisionReceiptSha256s != expected_receipts
        || value.publishReceiptSha256 != lower_hex(codec.sha256(&unsigned_bytes))
    {
        return Err(NativeRevisionColdEntryError::IdentityConflict);
    }
    Ok(value)
}

/// Map an authenticated database kind to the ordinary projection kind grammar.
fn projection_kind(
    value: AdjustmentRevisionCommitReceiptProjectionKindEnum,
) -> Option<AdjustmentRevisionProjectionKind> {
    // map closed wire kinds
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

/// Compare the distinct database and staged projection kind enums without relabelling.
fn projection_kind_matches(
    receipt: AdjustmentRevisionCommitReceiptProjectionKindEnum,
    projection: AdjustmentRevisionProjectionKind,
) -> bool {
    projection_kind(receipt) == Some(projection)
}

/// Map only the two producer kinds supported by grouped publication receipts.
fn batch_projection_kind(
    value: AdjustmentRevisionCommitReceiptProjectionKindEnum,
) -> Option<weather_contracts::AdjustmentRevisionBatchPublishReceiptProjectionKindEnum> {
    // admit grouped wire kinds
    match value {
        AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch => Some(
            weather_contracts::AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::ActualBestMatch,
        ),
        AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision => Some(
            weather_contracts::AdjustmentRevisionBatchPublishReceiptProjectionKindEnum::TargetRevision,
        ),
        _ => None,
    }
}

/// Keep each shadow capsule grammar paired with its original publication contract.
fn shadow_publish_version_matches(
    version: PrivateShadowArchiveVersion,
    contract: weather_contracts::AdjustmentMaintenancePublishReceiptContractVersionEnum,
) -> bool {
    matches!(
        (version, contract),
        (
            PrivateShadowArchiveVersion::V1,
            weather_contracts::AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV1
        ) | (
            PrivateShadowArchiveVersion::V2,
            weather_contracts::AdjustmentMaintenancePublishReceiptContractVersionEnum::AdjustmentShadowPublishReceiptV2
        )
    )
}
