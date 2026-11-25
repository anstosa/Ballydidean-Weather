//! Typed acknowledgement and ordered retirement for one frozen revision-gap page.

use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, instant,
};
use crate::revision_gap_transfer::{
    AdmittedRevisionGapPageIdentity, REVISION_GAP_PAGE_MAXIMUM_BYTES,
    REVISION_GAP_PAGE_MAXIMUM_ENTRIES, REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION,
    RetainedRevisionGapEntry, RevisionGapFrontier, RevisionGapIdentityAdapter,
    RevisionGapPayloadPage, RevisionGapSha256, RevisionGapTransferError, RevisionGapUnsignedPage,
    validate_retained_entry,
};
use crate::revision_online_spool::{RevisionOnlineSpoolError, RevisionOnlineSpoolStorage};

/// wire contract emitted by `revision-gap-ack-v1`.
pub const REVISION_GAP_ACK_CONTRACT_VERSION: &str = "adjustment-revision-gap-payload-ack/v1";
/// maximum canonical bytes retained for the compact current checkpoint.
pub const REVISION_GAP_ACK_MAXIMUM_BYTES: usize = 4_096;

/// one typed operator acknowledgement request after argument decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionGapAckRequest {
    pub graph_manifest_sha256: RevisionGapSha256,
    pub page_sha256: RevisionGapSha256,
}

/// one durable non-ordinal gap frontier checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionGapPayloadAcknowledgement {
    pub acknowledged_at: String,
    pub acknowledgement_sha256: RevisionGapSha256,
    pub contract_version: &'static str,
    pub frontier_sha256: RevisionGapSha256,
    pub graph_manifest_sha256: RevisionGapSha256,
    pub member_root_sha256: RevisionGapSha256,
    pub page_count: u64,
    pub page_sha256: RevisionGapSha256,
    pub predecessor_frontier_sha256: RevisionGapSha256,
}

/// exact unsigned acknowledgement fields passed to the canonical codec owner.
#[derive(Debug, Clone, Copy)]
pub struct RevisionGapUnsignedAcknowledgement<'a> {
    pub acknowledged_at: &'a str,
    pub contract_version: &'static str,
    pub graph_manifest_sha256: RevisionGapSha256,
    pub member_root_sha256: RevisionGapSha256,
    pub page_count: u64,
    pub page_sha256: RevisionGapSha256,
    pub predecessor_frontier_sha256: RevisionGapSha256,
}

/// both identities derived from one exact unsigned acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedRevisionGapAckIdentity {
    pub acknowledgement_sha256: RevisionGapSha256,
    pub frontier_sha256: RevisionGapSha256,
}

/// one checkpoint file decoded together with its original canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredRevisionGapCheckpoint {
    Genesis,
    Acknowledged {
        acknowledgement: Box<RevisionGapPayloadAcknowledgement>,
        canonical_bytes: Vec<u8>,
    },
}

/// one frozen pending page decoded together with its original canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRevisionGapPendingPage<T> {
    pub canonical_bytes: Vec<u8>,
    pub page: RevisionGapPayloadPage<T>,
}

/// one fixed private directory eligible for post-checkpoint retirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionGapRetirementDirectory {
    RainControlStateGaps,
    RainControlStateStageReceipts,
    RainControlStates,
    RevisionProjections,
    RevisionStageReceipts,
    ShadowRevisionStages,
}

impl RevisionGapRetirementDirectory {
    /// resolve only a source-owned fixed directory component.
    pub const fn directory(self) -> &'static str {
        match self {
            Self::RainControlStateGaps => "rain-control-state-gaps",
            Self::RainControlStateStageReceipts => "rain-control-state-stage-receipts",
            Self::RainControlStates => "rain-control-states",
            Self::RevisionProjections => "revision-projections",
            Self::RevisionStageReceipts => "revision-stage-receipts",
            Self::ShadowRevisionStages => "shadow-revision-stages",
        }
    }
}

/// one exact optional file whose retained bytes must match before deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionGapRetirementMember {
    pub directory: RevisionGapRetirementDirectory,
    pub expected_sha256: RevisionGapSha256,
    pub maximum_bytes: usize,
    pub slot_sha256: RevisionGapSha256,
}

/// one source-ordered verify-all, remove-all, then sync-all retirement group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionGapRetirementGroup {
    pub members: Vec<RevisionGapRetirementMember>,
}

/// exact canonical operations not reimplemented by the typed state machine.
pub trait RevisionGapAckCodec<T>:
    RevisionGapIdentityAdapter<T> + RevisionArchiveCanonicalCodec
{
    /// return the fixed empty revision-gap frontier identity.
    fn genesis_frontier_sha256(&self) -> RevisionGapSha256;

    /// encode one complete payload page in its exact canonical representation.
    fn payload_page_bytes(&self, page: &RevisionGapPayloadPage<T>) -> Option<Vec<u8>>;

    /// derive the compact frontier and acknowledgement identities.
    fn acknowledgement_identity(
        &self,
        value: RevisionGapUnsignedAcknowledgement<'_>,
    ) -> Option<AdmittedRevisionGapAckIdentity>;

    /// encode one complete acknowledgement in its exact canonical representation.
    fn acknowledgement_bytes(&self, value: &RevisionGapPayloadAcknowledgement) -> Option<Vec<u8>>;

    /// project one admitted page entry into its exact source retirement groups.
    fn retirement_groups(
        &self,
        entry: &RetainedRevisionGapEntry<T>,
    ) -> Option<Vec<RevisionGapRetirementGroup>>;
}

/// native operations kept below the typed acknowledgement and ordering owner.
pub trait RevisionGapAckStorage<T>: RevisionOnlineSpoolStorage {
    /// verify the fixed archive root and gap-transfer children before locking.
    fn ensure_revision_gap_archive(&mut self) -> Result<(), RevisionOnlineSpoolError>;

    /// read the sole current checkpoint or the implicit genesis state.
    fn read_revision_gap_checkpoint(
        &mut self,
        maximum_bytes: usize,
    ) -> Result<StoredRevisionGapCheckpoint, RevisionOnlineSpoolError>;

    /// read the sole frozen pending page when it exists.
    fn read_revision_gap_pending_page(
        &mut self,
        maximum_bytes: usize,
    ) -> Result<Option<StoredRevisionGapPendingPage<T>>, RevisionOnlineSpoolError>;

    /// atomically replace the compact current checkpoint with exact canonical bytes.
    fn write_revision_gap_checkpoint_atomic(
        &mut self,
        bytes: &[u8],
        maximum_bytes: usize,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// verify one optional retirement file by bounded content digest.
    fn verify_revision_gap_retirement_member(
        &mut self,
        member: RevisionGapRetirementMember,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// remove one already-verified retirement file while accepting prior absence.
    fn remove_revision_gap_retirement_member_if_present(
        &mut self,
        member: RevisionGapRetirementMember,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// make prior removals from one fixed retirement directory durable.
    fn sync_revision_gap_retirement_directory(
        &mut self,
        directory: RevisionGapRetirementDirectory,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// remove the pending page with fresh versus crash-retry absence semantics.
    fn remove_revision_gap_pending_page(
        &mut self,
        allow_absent: bool,
    ) -> Result<(), RevisionOnlineSpoolError>;

    /// make the pending-page removal durable after every retired page.
    fn sync_revision_gap_transfer_directory(&mut self) -> Result<(), RevisionOnlineSpoolError>;
}

/// closed ordinary failures from typed acknowledgement processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionGapAckError {
    AcknowledgementCollision,
    CheckpointDiffers,
    Clock,
    IdentityUnavailable,
    PageCountOverflow,
    PendingPageDiffers,
    PendingPageUnavailable,
    RetirementPlanDiffers,
    Storage,
}

impl From<RevisionOnlineSpoolError> for RevisionGapAckError {
    /// collapse native failures without claiming a successful checkpoint transition.
    fn from(_value: RevisionOnlineSpoolError) -> Self {
        Self::Storage
    }
}

/// acknowledge one exact frozen nonempty page and retire only its bound hot files.
pub fn acknowledge_revision_gap_payload<S, C, T>(
    storage: &mut S,
    codec: &mut C,
    clock: &mut impl RevisionArchiveCanonicalClock,
    request: RevisionGapAckRequest,
) -> Result<RevisionGapPayloadAcknowledgement, RevisionGapAckError>
where
    S: RevisionGapAckStorage<T>,
    C: RevisionGapAckCodec<T>,
{
    storage.ensure_revision_gap_archive()?;
    let mut domain_error = None;
    let result = storage.with_revision_spool_lease(
        // keep checkpoint validation and every retirement mutation under one spool lease
        |storage| {
            acknowledge_revision_gap_payload_locked(storage, codec, clock, request).map_err(
                // preserve the exact domain failure across the generic native lease boundary
                |error| {
                    domain_error = Some(error);
                    RevisionOnlineSpoolError::Storage
                },
            )
        },
    );
    // return an exact validation outcome before collapsing genuine native lease failures
    if let Some(error) = domain_error {
        return Err(error);
    }
    result.map_err(RevisionGapAckError::from)
}

/// execute the acknowledgement transition after the native lease is held.
fn acknowledge_revision_gap_payload_locked<S, C, T>(
    storage: &mut S,
    codec: &mut C,
    clock: &mut impl RevisionArchiveCanonicalClock,
    request: RevisionGapAckRequest,
) -> Result<RevisionGapPayloadAcknowledgement, RevisionGapAckError>
where
    S: RevisionGapAckStorage<T>,
    C: RevisionGapAckCodec<T>,
{
    let checkpoint = storage.read_revision_gap_checkpoint(REVISION_GAP_ACK_MAXIMUM_BYTES)?;
    let frontier = validate_checkpoint(codec, &checkpoint)?;
    let pending = storage.read_revision_gap_pending_page(REVISION_GAP_PAGE_MAXIMUM_BYTES)?;

    // converge only a retry for the byte-identical graph checkpoint
    if let StoredRevisionGapCheckpoint::Acknowledged {
        acknowledgement, ..
    } = &checkpoint
        && acknowledgement.page_sha256 == request.page_sha256
    {
        // preserve the first graph identity bound to this page
        if acknowledgement.graph_manifest_sha256 != request.graph_manifest_sha256 {
            return Err(RevisionGapAckError::AcknowledgementCollision);
        }
        // finish only a crash-interrupted retirement for this exact checkpoint
        if let Some(pending) = pending {
            validate_pending_page(codec, &pending, frontier, Some(acknowledgement))?;
            retire_revision_gap_page(storage, codec, &pending.page)?;
            storage.remove_revision_gap_pending_page(true)?;
            storage.sync_revision_gap_transfer_directory()?;
        }
        return Ok(acknowledgement.as_ref().clone());
    }

    let pending = pending.ok_or(RevisionGapAckError::PendingPageUnavailable)?;
    validate_pending_page(codec, &pending, frontier, None)?;
    // bind a fresh transition to the exact requested frozen page
    if pending.page.page_sha256 != request.page_sha256 {
        return Err(RevisionGapAckError::PendingPageDiffers);
    }
    let acknowledged_at = clock.canonical_now().ok_or(RevisionGapAckError::Clock)?;
    // reject a clock string the exact canonical timestamp owner cannot admit
    if instant(codec, &acknowledged_at).is_none() {
        return Err(RevisionGapAckError::Clock);
    }
    let page_count = frontier
        .page_count
        .checked_add(1)
        .ok_or(RevisionGapAckError::PageCountOverflow)?;
    let unsigned = RevisionGapUnsignedAcknowledgement {
        acknowledged_at: &acknowledged_at,
        contract_version: REVISION_GAP_ACK_CONTRACT_VERSION,
        graph_manifest_sha256: request.graph_manifest_sha256,
        member_root_sha256: pending.page.member_root_sha256,
        page_count,
        page_sha256: pending.page.page_sha256,
        predecessor_frontier_sha256: frontier.frontier_sha256,
    };
    let identity = codec
        .acknowledgement_identity(unsigned)
        .ok_or(RevisionGapAckError::IdentityUnavailable)?;
    let acknowledgement = RevisionGapPayloadAcknowledgement {
        acknowledged_at,
        acknowledgement_sha256: identity.acknowledgement_sha256,
        contract_version: REVISION_GAP_ACK_CONTRACT_VERSION,
        frontier_sha256: identity.frontier_sha256,
        graph_manifest_sha256: request.graph_manifest_sha256,
        member_root_sha256: pending.page.member_root_sha256,
        page_count,
        page_sha256: pending.page.page_sha256,
        predecessor_frontier_sha256: frontier.frontier_sha256,
    };
    let checkpoint_bytes = codec
        .acknowledgement_bytes(&acknowledgement)
        .filter(|bytes| bytes.len() <= REVISION_GAP_ACK_MAXIMUM_BYTES)
        .ok_or(RevisionGapAckError::IdentityUnavailable)?;
    storage
        .write_revision_gap_checkpoint_atomic(&checkpoint_bytes, REVISION_GAP_ACK_MAXIMUM_BYTES)?;
    retire_revision_gap_page(storage, codec, &pending.page)?;
    storage.remove_revision_gap_pending_page(false)?;
    storage.sync_revision_gap_transfer_directory()?;
    Ok(acknowledgement)
}

/// validate the implicit genesis or one exact canonical retained checkpoint.
fn validate_checkpoint<T>(
    codec: &impl RevisionGapAckCodec<T>,
    checkpoint: &StoredRevisionGapCheckpoint,
) -> Result<RevisionGapFrontier, RevisionGapAckError> {
    match checkpoint {
        StoredRevisionGapCheckpoint::Genesis => Ok(RevisionGapFrontier {
            frontier_sha256: codec.genesis_frontier_sha256(),
            page_count: 0,
        }),
        StoredRevisionGapCheckpoint::Acknowledged {
            acknowledgement,
            canonical_bytes,
        } => {
            // retain only the exact current acknowledgement contract and timestamp
            if acknowledgement.contract_version != REVISION_GAP_ACK_CONTRACT_VERSION
                || acknowledgement.page_count == 0
                || canonical_bytes.len() > REVISION_GAP_ACK_MAXIMUM_BYTES
                || instant(codec, &acknowledgement.acknowledged_at).is_none()
            {
                return Err(RevisionGapAckError::CheckpointDiffers);
            }
            let unsigned = RevisionGapUnsignedAcknowledgement {
                acknowledged_at: &acknowledgement.acknowledged_at,
                contract_version: acknowledgement.contract_version,
                graph_manifest_sha256: acknowledgement.graph_manifest_sha256,
                member_root_sha256: acknowledgement.member_root_sha256,
                page_count: acknowledgement.page_count,
                page_sha256: acknowledgement.page_sha256,
                predecessor_frontier_sha256: acknowledgement.predecessor_frontier_sha256,
            };
            let identity = codec
                .acknowledgement_identity(unsigned)
                .ok_or(RevisionGapAckError::CheckpointDiffers)?;
            let encoded = codec
                .acknowledgement_bytes(acknowledgement)
                .ok_or(RevisionGapAckError::CheckpointDiffers)?;
            // bind both checkpoint digests and its retained byte representation
            if identity.acknowledgement_sha256 != acknowledgement.acknowledgement_sha256
                || identity.frontier_sha256 != acknowledgement.frontier_sha256
                || encoded != *canonical_bytes
            {
                return Err(RevisionGapAckError::CheckpointDiffers);
            }
            Ok(RevisionGapFrontier {
                frontier_sha256: acknowledgement.frontier_sha256,
                page_count: acknowledgement.page_count,
            })
        }
    }
}

/// recompute one pending page and bind it to the current checkpoint.
fn validate_pending_page<T>(
    codec: &mut impl RevisionGapAckCodec<T>,
    pending: &StoredRevisionGapPendingPage<T>,
    frontier: RevisionGapFrontier,
    retry_checkpoint: Option<&RevisionGapPayloadAcknowledgement>,
) -> Result<(), RevisionGapAckError> {
    let page = &pending.page;
    let expected_predecessor = retry_checkpoint.map_or(
        frontier.frontier_sha256,
        // a crash-retained page necessarily precedes its already-written checkpoint
        |checkpoint| checkpoint.predecessor_frontier_sha256,
    );
    // require one bounded nonempty pending page for the current predecessor
    if page.contract_version != REVISION_GAP_PAYLOAD_PAGE_CONTRACT_VERSION
        || page.entries.is_empty()
        || page.entries.len() > REVISION_GAP_PAGE_MAXIMUM_ENTRIES
        || page.idle
        || page.canonical_byte_count != pending.canonical_bytes.len()
        || pending.canonical_bytes.len() > REVISION_GAP_PAGE_MAXIMUM_BYTES
        || page.predecessor_frontier_sha256 != expected_predecessor
    {
        return Err(RevisionGapAckError::PendingPageDiffers);
    }
    // reapply every individual source read ceiling before retirement begins
    for entry in &page.entries {
        validate_retained_entry(entry).map_err(map_transfer_identity_error)?;
    }
    let identity = codec
        .page_identity(RevisionGapUnsignedPage {
            contract_version: page.contract_version,
            entries: &page.entries,
            idle: page.idle,
            predecessor_frontier_sha256: page.predecessor_frontier_sha256,
            start_sha256: page.start_sha256,
        })
        .map_err(map_transfer_identity_error)?;
    let encoded = codec
        .payload_page_bytes(page)
        .ok_or(RevisionGapAckError::PendingPageDiffers)?;
    // bind the retained bytes, member root, and page digest to one canonical page
    if !page_identity_matches(page, identity)
        || encoded != pending.canonical_bytes
        || retry_checkpoint.is_some_and(|checkpoint| {
            checkpoint.page_sha256 != page.page_sha256
                || checkpoint.member_root_sha256 != page.member_root_sha256
                || checkpoint.predecessor_frontier_sha256 != page.predecessor_frontier_sha256
        })
    {
        return Err(RevisionGapAckError::PendingPageDiffers);
    }
    Ok(())
}

/// compare every page identity field returned by the canonical owner.
fn page_identity_matches<T>(
    page: &RevisionGapPayloadPage<T>,
    identity: AdmittedRevisionGapPageIdentity,
) -> bool {
    identity.canonical_byte_count == page.canonical_byte_count
        && identity.member_root_sha256 == page.member_root_sha256
        && identity.page_sha256 == page.page_sha256
}

/// retire each entry using the source verify, remove, then directory-sync ordering.
fn retire_revision_gap_page<S, C, T>(
    storage: &mut S,
    codec: &C,
    page: &RevisionGapPayloadPage<T>,
) -> Result<(), RevisionGapAckError>
where
    S: RevisionGapAckStorage<T>,
    C: RevisionGapAckCodec<T>,
{
    // preserve page order because each source entry is retired independently
    for entry in &page.entries {
        let groups = codec
            .retirement_groups(entry)
            .ok_or(RevisionGapAckError::RetirementPlanDiffers)?;
        // preserve control-projection auxiliary retirement after its main projection group
        for group in groups {
            if group.members.is_empty() {
                return Err(RevisionGapAckError::RetirementPlanDiffers);
            }
            // verify the complete source group before deleting its first member
            for member in &group.members {
                storage.verify_revision_gap_retirement_member(*member)?;
            }
            // accept only prior absence caused by a post-checkpoint crash retry
            for member in &group.members {
                storage.remove_revision_gap_retirement_member_if_present(*member)?;
            }
            // make each source directory removal durable in member order
            for member in &group.members {
                storage.sync_revision_gap_retirement_directory(member.directory)?;
            }
        }
    }
    Ok(())
}

/// collapse page-identity failures into the exact pending-page outcome.
fn map_transfer_identity_error(_error: RevisionGapTransferError) -> RevisionGapAckError {
    RevisionGapAckError::PendingPageDiffers
}
