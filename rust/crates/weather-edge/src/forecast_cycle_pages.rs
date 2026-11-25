//! Pure bounded append and acknowledgement state for forecast evidence cycle pages.

use std::collections::HashSet;

use weather_contracts::{
    ForecastArchivePageAcknowledgementV1ContractVersionLiteral,
    ForecastArchivePageHeaderV1ContractVersionLiteral,
};

use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant};

/// exact online page contract retained by the Node owner.
pub const FORECAST_CYCLE_PAGE_CONTRACT_VERSION: &str =
    ForecastArchivePageHeaderV1ContractVersionLiteral::WIRE_VALUE;
/// exact page acknowledgement contract retained by the Node owner.
pub const FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION: &str =
    ForecastArchivePageAcknowledgementV1ContractVersionLiteral::WIRE_VALUE;
/// exact in-memory state contract retained across recovery operations.
pub const FORECAST_CYCLE_PAGE_STATE_CONTRACT_VERSION: &str = "adjustment-cycle-page-state/v1";
/// maximum payload bytes in one current or next online page.
pub const FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES: usize = 256 * 1_024;
/// maximum page count in one causal cycle.
pub const FORECAST_CYCLE_MAXIMUM_PAGES: usize = 19;
/// maximum page count admitted across one local date.
pub const FORECAST_CYCLE_DAILY_MAXIMUM_PAGES: usize = 22;
/// maximum total projection count admitted across one local date.
pub const FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS: usize = 4_096;
/// projection capacity reserved exclusively for scheduler requests.
pub const FORECAST_CYCLE_RESERVED_SCHEDULER_PROJECTIONS: usize = 4;

/// one digest admitted by the approved archive codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForecastCycleSha256([u8; 32]);

impl ForecastCycleSha256 {
    /// retain one digest returned by an approved codec or decoder.
    pub const fn admitted(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// borrow the exact digest bytes for canonical wire encoding.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// closed channel classes counted independently by page admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCycleProjectionChannel {
    Organic,
    SchedulerRequest,
}

impl ForecastCycleProjectionChannel {
    /// return the exact source wire spelling.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Organic => "organic",
            Self::SchedulerRequest => "scheduler_request",
        }
    }
}

/// one immutable projection reference normalized into digest order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastCycleProjection {
    pub channel: ForecastCycleProjectionChannel,
    pub identity_sha256: ForecastCycleSha256,
}

/// typed online page header encoded by the approved canonical codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCyclePageHeader {
    pub contract_version: &'static str,
    pub due_key: String,
    pub generation: u64,
    pub page_index: usize,
    pub predecessor_page_sha256: Option<ForecastCycleSha256>,
    pub payload_sha256: ForecastCycleSha256,
    pub payload_offset: u64,
}

/// immutable metadata retained after one page payload becomes durable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCyclePage {
    pub page_index: usize,
    pub page_sha256: ForecastCycleSha256,
    pub predecessor_page_sha256: Option<ForecastCycleSha256>,
    pub payload_sha256: ForecastCycleSha256,
    pub payload_offset: u64,
    pub payload_length: usize,
    pub projection_identities: Vec<ForecastCycleSha256>,
    pub projection_channels: Vec<ForecastCycleProjectionChannel>,
}

/// fixed physical online slot names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCyclePageSlotKind {
    Current,
    Next,
}

impl ForecastCyclePageSlotKind {
    /// return the exact fixed filesystem slot spelling.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Next => "next",
        }
    }
}

/// one pending page projected into its fixed current or next slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastCyclePageSlot {
    pub page_index: usize,
    pub page_sha256: ForecastCycleSha256,
    pub slot: ForecastCyclePageSlotKind,
}

/// exact acknowledgement fields hashed before durable publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCyclePageAcknowledgement {
    pub contract_version: &'static str,
    pub due_key: String,
    pub generation: u64,
    pub page_index: usize,
    pub page_sha256: ForecastCycleSha256,
    pub acknowledged_at: String,
}

/// one acknowledgement retained only after an exact fsync proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCycleDurableAcknowledgement {
    pub acknowledgement: ForecastCyclePageAcknowledgement,
    pub acknowledgement_sha256: ForecastCycleSha256,
}

/// complete append frontier plus opaque later gap and finalization records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCyclePageState<Gap = (), Finalized = ()> {
    pub contract_version: &'static str,
    pub due_key: String,
    pub generation: u64,
    pub local_date: String,
    pub daily_page_count_at_open: usize,
    pub pages: Vec<ForecastCyclePage>,
    pub acknowledgements: Vec<ForecastCycleDurableAcknowledgement>,
    pub(crate) gaps: Vec<Gap>,
    pub slots: Vec<ForecastCyclePageSlot>,
    pub organic_projection_count: usize,
    pub scheduler_projection_count: usize,
    pub(crate) finalized: Option<Finalized>,
}

impl<Gap, Finalized> ForecastCyclePageState<Gap, Finalized> {
    /// Expose retained evidence without letting callers install unvalidated gap records.
    pub fn gaps(&self) -> &[Gap] {
        &self.gaps
    }

    /// Install independently admitted native extensions without changing the retained frontier.
    pub(crate) fn with_validated_gaps<ValidatedGap>(
        self,
        gaps: Vec<ValidatedGap>,
    ) -> ForecastCyclePageState<ValidatedGap, Finalized> {
        ForecastCyclePageState {
            contract_version: self.contract_version,
            due_key: self.due_key,
            generation: self.generation,
            local_date: self.local_date,
            daily_page_count_at_open: self.daily_page_count_at_open,
            pages: self.pages,
            acknowledgements: self.acknowledgements,
            gaps,
            slots: self.slots,
            organic_projection_count: self.organic_projection_count,
            scheduler_projection_count: self.scheduler_projection_count,
            finalized: self.finalized,
        }
    }

    /// Mark a validated frontier finalized only after the native owner's fsync.
    pub(crate) fn with_final_manifest<ValidatedFinal>(
        self,
        manifest: ValidatedFinal,
    ) -> ForecastCyclePageState<Gap, ValidatedFinal> {
        ForecastCyclePageState {
            contract_version: self.contract_version,
            due_key: self.due_key,
            generation: self.generation,
            local_date: self.local_date,
            daily_page_count_at_open: self.daily_page_count_at_open,
            pages: self.pages,
            acknowledgements: self.acknowledgements,
            gaps: self.gaps,
            slots: self.slots,
            organic_projection_count: self.organic_projection_count,
            scheduler_projection_count: self.scheduler_projection_count,
            finalized: Some(manifest),
        }
    }

    /// Expose whether the retained cycle is already closed, without reopening it.
    pub const fn is_finalized(&self) -> bool {
        self.finalized.is_some()
    }
}

/// already decoded input for a new empty cycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCyclePageStateInput {
    pub daily_page_count: usize,
    pub due_key: String,
    pub generation: u64,
    pub local_date: String,
}

/// borrowed payload and typed projection set for one append attempt.
#[derive(Debug, Clone, Copy)]
pub struct ForecastCyclePageAppendInput<'a> {
    pub payload: &'a [u8],
    pub projections: &'a [ForecastCycleProjection],
}

/// one exact acknowledgement request for the oldest pending page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCyclePageAckInput {
    pub acknowledged_at: String,
    pub page_sha256: ForecastCycleSha256,
}

/// exact canonical encodings and two-segment hashing retained outside this state machine.
pub trait ForecastCyclePageCodec: RevisionArchiveCanonicalCodec {
    /// encode the exact canonical newline-framed page header.
    fn encode_page_header(&self, header: &ForecastCyclePageHeader) -> Option<Vec<u8>>;

    /// encode the exact canonical newline-framed acknowledgement without its digest.
    fn encode_page_acknowledgement(
        &self,
        acknowledgement: &ForecastCyclePageAcknowledgement,
    ) -> Option<Vec<u8>>;

    /// hash two exact byte segments without forcing an intermediate allocation.
    fn sha256_pair(&self, first: &[u8], second: &[u8]) -> [u8; 32];
}

/// borrowed exact page material supplied to its durability owner.
pub struct ForecastCyclePagePersistenceRequest<'a> {
    pub header: &'a ForecastCyclePageHeader,
    pub page_sha256: ForecastCycleSha256,
    pub payload: &'a [u8],
    pub projections: &'a [ForecastCycleProjection],
    pub slot: ForecastCyclePageSlotKind,
}

/// exact page fsync receipt returned by the durability owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastCyclePageDurabilityProof {
    pub fsynced: bool,
    pub page_sha256: ForecastCycleSha256,
}

/// persistence boundary for one current or next immutable page.
pub trait ForecastCyclePagePersistence {
    type Error;

    /// durably publish one page and return its exact fsync receipt.
    fn persist_page(
        &mut self,
        request: ForecastCyclePagePersistenceRequest<'_>,
    ) -> Result<ForecastCyclePageDurabilityProof, Self::Error>;
}

/// borrowed acknowledgement and promoted slot frontier supplied to durability.
pub struct ForecastCyclePageAckPersistenceRequest<'a> {
    pub acknowledgement: &'a ForecastCyclePageAcknowledgement,
    pub acknowledgement_sha256: ForecastCycleSha256,
    pub next_slots: &'a [ForecastCyclePageSlot],
}

/// exact acknowledgement fsync receipt returned by the durability owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastCyclePageAckDurabilityProof {
    pub fsynced: bool,
    pub acknowledgement_sha256: ForecastCycleSha256,
}

/// persistence boundary for one acknowledgement and slot promotion.
pub trait ForecastCyclePageAckPersistence {
    type Error;

    /// durably publish one acknowledgement before exposing its next frontier.
    fn persist_acknowledgement(
        &mut self,
        request: ForecastCyclePageAckPersistenceRequest<'_>,
    ) -> Result<ForecastCyclePageAckDurabilityProof, Self::Error>;
}

/// closed source refusal reasons from append and acknowledgement admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCyclePageRefusal {
    CycleAlreadyFinalized,
    SlotUnavailable,
    PagePayloadRefused,
    ProjectionIdentityCollision,
    PageLimit,
    ProjectionLimit,
    AcknowledgementCollision,
    AcknowledgementOrderRefused,
    DurabilityProofRefused,
}

impl ForecastCyclePageRefusal {
    /// return the exact Node reason string.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::CycleAlreadyFinalized => "cycle_already_finalized",
            Self::SlotUnavailable => "slot_unavailable",
            Self::PagePayloadRefused => "page_payload_refused",
            Self::ProjectionIdentityCollision => "projection_identity_collision",
            Self::PageLimit => "page_limit",
            Self::ProjectionLimit => "projection_limit",
            Self::AcknowledgementCollision => "acknowledgement_collision",
            Self::AcknowledgementOrderRefused => "acknowledgement_order_refused",
            Self::DurabilityProofRefused => "durability_proof_refused",
        }
    }
}

/// typed state, codec, persistence, or ordinary refusal result.
#[derive(Debug, PartialEq, Eq)]
pub enum ForecastCyclePageError<StorageError> {
    InvalidState,
    InvalidInput,
    Codec,
    Persistence(StorageError),
    Refused(ForecastCyclePageRefusal),
}

/// create one empty bounded online page chain from admitted scalar input.
pub fn create_forecast_cycle_page_state(
    input: ForecastCyclePageStateInput,
) -> Option<ForecastCyclePageState> {
    // retain only the source due-key, local-date, and open daily page bounds
    if !valid_due_key(&input.due_key)
        || !valid_local_date(&input.local_date)
        || input.daily_page_count >= FORECAST_CYCLE_DAILY_MAXIMUM_PAGES
    {
        return None;
    }
    Some(ForecastCyclePageState {
        contract_version: FORECAST_CYCLE_PAGE_STATE_CONTRACT_VERSION,
        due_key: input.due_key,
        generation: input.generation,
        local_date: input.local_date,
        daily_page_count_at_open: input.daily_page_count,
        pages: Vec::new(),
        acknowledgements: Vec::new(),
        gaps: Vec::new(),
        slots: Vec::new(),
        organic_projection_count: 0,
        scheduler_projection_count: 0,
        finalized: None,
    })
}

/// append one predecessor-bound page only after its exact fsync receipt.
pub fn append_forecast_cycle_page<Gap, Finalized, Codec, Persistence>(
    state: &ForecastCyclePageState<Gap, Finalized>,
    input: ForecastCyclePageAppendInput<'_>,
    codec: &Codec,
    persistence: &mut Persistence,
) -> Result<ForecastCyclePageState<Gap, Finalized>, ForecastCyclePageError<Persistence::Error>>
where
    Gap: Clone,
    Finalized: Clone,
    Codec: ForecastCyclePageCodec,
    Persistence: ForecastCyclePagePersistence,
{
    use ForecastCyclePageError as Error;
    use ForecastCyclePageRefusal as Refusal;

    // reject a corrupted frontier before deriving any new hash or persistence request
    if !valid_state(state, codec) {
        return Err(Error::InvalidState);
    }
    let projections = normalize_projections(input.projections).ok_or(Error::InvalidInput)?;
    let existing_projection_identities = state
        .pages
        .iter()
        .flat_map(|page| page.projection_identities.iter().copied())
        .collect::<HashSet<_>>();

    // preserve the source refusal precedence after structural input validation
    if state.finalized.is_some() {
        return Err(Error::Refused(Refusal::CycleAlreadyFinalized));
    }
    if state.slots.len() >= 2 {
        return Err(Error::Refused(Refusal::SlotUnavailable));
    }
    if input.payload.is_empty() || input.payload.len() > FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES {
        return Err(Error::Refused(Refusal::PagePayloadRefused));
    }
    if projections
        .iter()
        .any(|projection| existing_projection_identities.contains(&projection.identity_sha256))
    {
        return Err(Error::Refused(Refusal::ProjectionIdentityCollision));
    }
    if state.pages.len() >= FORECAST_CYCLE_MAXIMUM_PAGES
        || state.daily_page_count_at_open + state.pages.len() >= FORECAST_CYCLE_DAILY_MAXIMUM_PAGES
    {
        return Err(Error::Refused(Refusal::PageLimit));
    }
    let (organic, scheduler) = projection_counts(&projections);
    let next_organic_count = state
        .organic_projection_count
        .checked_add(organic)
        .ok_or(Error::InvalidState)?;
    let next_scheduler_count = state
        .scheduler_projection_count
        .checked_add(scheduler)
        .ok_or(Error::InvalidState)?;

    // preserve four scheduler reservations before admitting ordinary projections
    if next_organic_count
        > FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS - FORECAST_CYCLE_RESERVED_SCHEDULER_PROJECTIONS
        || next_scheduler_count > FORECAST_CYCLE_RESERVED_SCHEDULER_PROJECTIONS
        || next_organic_count + next_scheduler_count > FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS
    {
        return Err(Error::Refused(Refusal::ProjectionLimit));
    }
    let previous = state.pages.last();
    let payload_offset = previous.map_or(Ok(0), |page| {
        page.payload_offset
            .checked_add(page.payload_length as u64)
            .ok_or(Error::InvalidState)
    })?;
    let payload_sha256 = ForecastCycleSha256::admitted(codec.sha256(input.payload));
    let header = ForecastCyclePageHeader {
        contract_version: FORECAST_CYCLE_PAGE_CONTRACT_VERSION,
        due_key: state.due_key.clone(),
        generation: state.generation,
        page_index: state.pages.len(),
        predecessor_page_sha256: previous.map(|page| page.page_sha256),
        payload_sha256,
        payload_offset,
    };
    let header_bytes = codec.encode_page_header(&header).ok_or(Error::Codec)?;
    let page_sha256 =
        ForecastCycleSha256::admitted(codec.sha256_pair(&header_bytes, input.payload));
    let slot = if state.slots.is_empty() {
        ForecastCyclePageSlotKind::Current
    } else {
        ForecastCyclePageSlotKind::Next
    };
    let proof = persistence
        .persist_page(ForecastCyclePagePersistenceRequest {
            header: &header,
            page_sha256,
            payload: input.payload,
            projections: &projections,
            slot,
        })
        .map_err(Error::Persistence)?;

    // expose no state change unless the persistence owner confirms the exact digest
    if !proof.fsynced || proof.page_sha256 != page_sha256 {
        return Err(Error::Refused(Refusal::DurabilityProofRefused));
    }
    let page = ForecastCyclePage {
        page_index: header.page_index,
        page_sha256,
        predecessor_page_sha256: header.predecessor_page_sha256,
        payload_sha256,
        payload_offset,
        payload_length: input.payload.len(),
        projection_identities: projections
            .iter()
            .map(|projection| projection.identity_sha256)
            .collect(),
        projection_channels: projections
            .iter()
            .map(|projection| projection.channel)
            .collect(),
    };
    let mut next = state.clone();
    next.pages.push(page);
    next.slots.push(ForecastCyclePageSlot {
        page_index: header.page_index,
        page_sha256,
        slot,
    });
    next.organic_projection_count = next_organic_count;
    next.scheduler_projection_count = next_scheduler_count;
    Ok(next)
}

/// acknowledge only the oldest pending page after its exact fsync receipt.
pub fn ack_forecast_cycle_page<Gap, Finalized, Codec, Persistence>(
    state: &ForecastCyclePageState<Gap, Finalized>,
    input: &ForecastCyclePageAckInput,
    codec: &Codec,
    persistence: &mut Persistence,
) -> Result<ForecastCyclePageState<Gap, Finalized>, ForecastCyclePageError<Persistence::Error>>
where
    Gap: Clone,
    Finalized: Clone,
    Codec: ForecastCyclePageCodec,
    Persistence: ForecastCyclePageAckPersistence,
{
    use ForecastCyclePageError as Error;
    use ForecastCyclePageRefusal as Refusal;

    // reject invalid retained or request clocks before inspecting acknowledgement order
    if !valid_state(state, codec) {
        return Err(Error::InvalidState);
    }
    if instant(codec, &input.acknowledged_at).is_none() {
        return Err(Error::InvalidInput);
    }
    let oldest = state.slots.first();

    // preserve exact idempotency only for a matching prior acknowledgement instant
    if oldest.is_none_or(|slot| slot.page_sha256 != input.page_sha256) {
        if let Some(existing) = state.acknowledgements.iter().find(|acknowledgement| {
            acknowledgement.acknowledgement.page_sha256 == input.page_sha256
        }) {
            if existing.acknowledgement.acknowledged_at != input.acknowledged_at {
                return Err(Error::Refused(Refusal::AcknowledgementCollision));
            }
            return Ok(state.clone());
        }
        return Err(Error::Refused(Refusal::AcknowledgementOrderRefused));
    }
    let oldest = oldest.ok_or(Error::InvalidState)?;
    let page = state
        .pages
        .get(oldest.page_index)
        .ok_or(Error::InvalidState)?;
    let acknowledgement = ForecastCyclePageAcknowledgement {
        contract_version: FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION,
        due_key: state.due_key.clone(),
        generation: state.generation,
        page_index: page.page_index,
        page_sha256: page.page_sha256,
        acknowledged_at: input.acknowledged_at.clone(),
    };
    let acknowledgement_bytes = codec
        .encode_page_acknowledgement(&acknowledgement)
        .ok_or(Error::Codec)?;
    let acknowledgement_sha256 =
        ForecastCycleSha256::admitted(codec.sha256(&acknowledgement_bytes));
    let next_slots = state
        .slots
        .iter()
        .skip(1)
        .map(
            // promote only an already durable next page into the current slot
            |slot| ForecastCyclePageSlot {
                slot: ForecastCyclePageSlotKind::Current,
                ..*slot
            },
        )
        .collect::<Vec<_>>();
    let proof = persistence
        .persist_acknowledgement(ForecastCyclePageAckPersistenceRequest {
            acknowledgement: &acknowledgement,
            acknowledgement_sha256,
            next_slots: &next_slots,
        })
        .map_err(Error::Persistence)?;

    // expose no frontier promotion unless the exact acknowledgement is durable
    if !proof.fsynced || proof.acknowledgement_sha256 != acknowledgement_sha256 {
        return Err(Error::Refused(Refusal::DurabilityProofRefused));
    }
    let mut next = state.clone();
    next.acknowledgements
        .push(ForecastCycleDurableAcknowledgement {
            acknowledgement,
            acknowledgement_sha256,
        });
    next.slots = next_slots;
    Ok(next)
}

/// validate the complete page, acknowledgement, slot, and count frontier.
pub(crate) fn valid_state<Gap, Finalized>(
    state: &ForecastCyclePageState<Gap, Finalized>,
    codec: &impl ForecastCyclePageCodec,
) -> bool {
    let Some(daily_page_count) = state
        .daily_page_count_at_open
        .checked_add(state.pages.len())
    else {
        return false;
    };
    // enforce the fixed scalar and collection bounds before indexing any frontier
    if state.contract_version != FORECAST_CYCLE_PAGE_STATE_CONTRACT_VERSION
        || !valid_due_key(&state.due_key)
        || !valid_local_date(&state.local_date)
        || state.pages.len() > FORECAST_CYCLE_MAXIMUM_PAGES
        || state.acknowledgements.len() > state.pages.len()
        || state.slots.len() > 2
        || daily_page_count > FORECAST_CYCLE_DAILY_MAXIMUM_PAGES
        || state.slots.len() != state.pages.len() - state.acknowledgements.len()
    {
        return false;
    }
    let mut expected_offset = 0_u64;
    let mut predecessor = None;
    let mut identities = HashSet::new();
    let mut organic = 0_usize;
    let mut scheduler = 0_usize;

    // validate one consecutive predecessor and payload-offset chain
    for (index, page) in state.pages.iter().enumerate() {
        if page.page_index != index
            || page.predecessor_page_sha256 != predecessor
            || page.payload_offset != expected_offset
            || page.payload_length == 0
            || page.payload_length > FORECAST_CYCLE_PAGE_PAYLOAD_MAXIMUM_BYTES
            || page.projection_identities.len() != page.projection_channels.len()
        {
            return false;
        }
        // retain every immutable projection identity exactly once
        for (identity, channel) in page
            .projection_identities
            .iter()
            .copied()
            .zip(page.projection_channels.iter().copied())
        {
            if !identities.insert(identity) {
                return false;
            }
            match channel {
                ForecastCycleProjectionChannel::Organic => organic += 1,
                ForecastCycleProjectionChannel::SchedulerRequest => scheduler += 1,
            }
        }
        let Some(next_offset) = expected_offset.checked_add(page.payload_length as u64) else {
            return false;
        };
        expected_offset = next_offset;
        predecessor = Some(page.page_sha256);
    }
    // validate the complete acknowledgement prefix and its exact canonical digest
    for (index, durable) in state.acknowledgements.iter().enumerate() {
        let acknowledgement = &durable.acknowledgement;
        let Some(page) = state.pages.get(index) else {
            return false;
        };
        if acknowledgement.contract_version != FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION
            || acknowledgement.due_key != state.due_key
            || acknowledgement.generation != state.generation
            || acknowledgement.page_index != index
            || acknowledgement.page_sha256 != page.page_sha256
            || instant(codec, &acknowledgement.acknowledged_at).is_none()
        {
            return false;
        }
        let Some(bytes) = codec.encode_page_acknowledgement(acknowledgement) else {
            return false;
        };
        if ForecastCycleSha256::admitted(codec.sha256(&bytes)) != durable.acknowledgement_sha256 {
            return false;
        }
    }
    // bind each pending page to its exact current or next physical slot
    for (index, slot) in state.slots.iter().enumerate() {
        let page_index = state.acknowledgements.len() + index;
        let Some(page) = state.pages.get(page_index) else {
            return false;
        };
        let expected_slot = if index == 0 {
            ForecastCyclePageSlotKind::Current
        } else {
            ForecastCyclePageSlotKind::Next
        };
        if slot.page_index != page_index
            || slot.page_sha256 != page.page_sha256
            || slot.slot != expected_slot
        {
            return false;
        }
    }
    organic == state.organic_projection_count
        && scheduler == state.scheduler_projection_count
        && organic
            <= FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS
                - FORECAST_CYCLE_RESERVED_SCHEDULER_PROJECTIONS
        && scheduler <= FORECAST_CYCLE_RESERVED_SCHEDULER_PROJECTIONS
}

/// normalize projection identity order and reject one duplicate input identity.
fn normalize_projections(
    projections: &[ForecastCycleProjection],
) -> Option<Vec<ForecastCycleProjection>> {
    // retain the source list ceiling before sorting a caller-owned projection set
    if projections.len() > FORECAST_CYCLE_DAILY_MAXIMUM_PROJECTIONS {
        return None;
    }
    let mut normalized = projections.to_vec();
    normalized.sort_by_key(|projection| projection.identity_sha256);
    // reject adjacent duplicate identities after deterministic normalization
    if normalized
        .windows(2)
        .any(|pair| pair[0].identity_sha256 == pair[1].identity_sha256)
    {
        return None;
    }
    Some(normalized)
}

/// count ordinary and scheduler projection channels independently.
fn projection_counts(projections: &[ForecastCycleProjection]) -> (usize, usize) {
    projections.iter().fold(
        (0, 0),
        // increment only the selected closed channel class
        |(organic, scheduler), projection| match projection.channel {
            ForecastCycleProjectionChannel::Organic => (organic + 1, scheduler),
            ForecastCycleProjectionChannel::SchedulerRequest => (organic, scheduler + 1),
        },
    )
}

/// match the exact source due-key regex without adding calendar semantics.
pub(crate) fn valid_due_key(value: &str) -> bool {
    let Some(value) = value.strip_prefix("capture/") else {
        return false;
    };
    let bytes = value.as_bytes();
    // preserve fixed punctuation, digits, and four scheduled UTC hours
    bytes.len() == 24
        && bytes.iter().enumerate().all(
            // admit digits only at the source regex's numeric positions
            |(index, byte)| match index {
                4 | 7 => *byte == b'-',
                10 => *byte == b'T',
                13 | 16 => *byte == b':',
                19 => *byte == b'.',
                23 => *byte == b'Z',
                _ => byte.is_ascii_digit(),
            },
        )
        && matches!(&bytes[11..13], b"00" | b"06" | b"12" | b"18")
        && &bytes[14..19] == b"35:00"
        && &bytes[20..23] == b"000"
}

/// match the exact source local-date regex without adding calendar semantics.
fn valid_local_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    // preserve four year digits and two digits for both month and day
    bytes.len() == 10
        && bytes.iter().enumerate().all(
            // admit only the two fixed date separators
            |(index, byte)| match index {
                4 | 7 => *byte == b'-',
                _ => byte.is_ascii_digit(),
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUE_KEY: &str = "capture/2026-11-23T00:35:00.000Z";
    const ACK_ONE: &str = "2026-11-23T00:36:00.000Z";
    const ACK_TWO: &str = "2026-11-23T00:37:00.000Z";

    struct FixtureCodec;

    impl RevisionArchiveCanonicalCodec for FixtureCodec {
        /// admit only the two exact acknowledgement clocks used by this fixture.
        fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
            match text {
                ACK_ONE => Some(1_795_394_160_000),
                ACK_TWO => Some(1_795_394_220_000),
                _ => None,
            }
        }

        /// hash one exact byte segment with the deterministic fixture owner.
        fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
            fixture_hash(&[bytes])
        }
    }

    impl ForecastCyclePageCodec for FixtureCodec {
        /// encode every typed header field into one deterministic fixture frame.
        fn encode_page_header(&self, header: &ForecastCyclePageHeader) -> Option<Vec<u8>> {
            Some(
                format!(
                    "{}|{}|{}|{}|{}|{}|{}\n",
                    header.contract_version,
                    header.due_key,
                    header.generation,
                    header.page_index,
                    header
                        .predecessor_page_sha256
                        .map_or(255, |digest| digest.as_bytes()[0]),
                    header.payload_sha256.as_bytes()[0],
                    header.payload_offset,
                )
                .into_bytes(),
            )
        }

        /// encode every typed acknowledgement field into one deterministic fixture frame.
        fn encode_page_acknowledgement(
            &self,
            acknowledgement: &ForecastCyclePageAcknowledgement,
        ) -> Option<Vec<u8>> {
            Some(
                format!(
                    "{}|{}|{}|{}|{}|{}\n",
                    acknowledgement.contract_version,
                    acknowledgement.due_key,
                    acknowledgement.generation,
                    acknowledgement.page_index,
                    acknowledgement.page_sha256.as_bytes()[0],
                    acknowledgement.acknowledged_at,
                )
                .into_bytes(),
            )
        }

        /// hash two exact byte segments without concatenating them first.
        fn sha256_pair(&self, first: &[u8], second: &[u8]) -> [u8; 32] {
            fixture_hash(&[first, second])
        }
    }

    #[derive(Debug, Clone)]
    struct PersistedPage {
        header: ForecastCyclePageHeader,
        page_sha256: ForecastCycleSha256,
        payload: Vec<u8>,
        projections: Vec<ForecastCycleProjection>,
        slot: ForecastCyclePageSlotKind,
    }

    #[derive(Default)]
    struct PagePersistence {
        pages: Vec<PersistedPage>,
        override_proof: Option<ForecastCyclePageDurabilityProof>,
    }

    impl ForecastCyclePagePersistence for PagePersistence {
        type Error = &'static str;

        /// retain the complete request and return its selected fsync proof.
        fn persist_page(
            &mut self,
            request: ForecastCyclePagePersistenceRequest<'_>,
        ) -> Result<ForecastCyclePageDurabilityProof, Self::Error> {
            self.pages.push(PersistedPage {
                header: request.header.clone(),
                page_sha256: request.page_sha256,
                payload: request.payload.to_vec(),
                projections: request.projections.to_vec(),
                slot: request.slot,
            });
            Ok(self
                .override_proof
                .unwrap_or(ForecastCyclePageDurabilityProof {
                    fsynced: true,
                    page_sha256: request.page_sha256,
                }))
        }
    }

    #[derive(Debug, Clone)]
    struct PersistedAck {
        acknowledgement: ForecastCyclePageAcknowledgement,
        acknowledgement_sha256: ForecastCycleSha256,
        next_slots: Vec<ForecastCyclePageSlot>,
    }

    #[derive(Default)]
    struct AckPersistence {
        acknowledgements: Vec<PersistedAck>,
        override_proof: Option<ForecastCyclePageAckDurabilityProof>,
    }

    impl ForecastCyclePageAckPersistence for AckPersistence {
        type Error = &'static str;

        /// retain the acknowledgement and return its selected fsync proof.
        fn persist_acknowledgement(
            &mut self,
            request: ForecastCyclePageAckPersistenceRequest<'_>,
        ) -> Result<ForecastCyclePageAckDurabilityProof, Self::Error> {
            self.acknowledgements.push(PersistedAck {
                acknowledgement: request.acknowledgement.clone(),
                acknowledgement_sha256: request.acknowledgement_sha256,
                next_slots: request.next_slots.to_vec(),
            });
            Ok(self
                .override_proof
                .unwrap_or(ForecastCyclePageAckDurabilityProof {
                    fsynced: true,
                    acknowledgement_sha256: request.acknowledgement_sha256,
                }))
        }
    }

    /// hash fixture segments in their exact streaming order.
    fn fixture_hash(parts: &[&[u8]]) -> [u8; 32] {
        let mut digest = [0_u8; 32];
        let mut index = 0_usize;
        // retain segment and byte order without an intermediate allocation
        for part in parts {
            for byte in *part {
                let slot = index % digest.len();
                digest[slot] = digest[slot].wrapping_mul(31).wrapping_add(*byte);
                index += 1;
            }
        }
        digest[31] ^= u8::try_from(index % 251).expect("bounded fixture remainder");
        digest
    }

    /// construct one repeated digest identity.
    fn digest(value: u8) -> ForecastCycleSha256 {
        ForecastCycleSha256::admitted([value; 32])
    }

    /// construct one ordinary empty cycle frontier.
    fn state() -> ForecastCyclePageState {
        create_forecast_cycle_page_state(ForecastCyclePageStateInput {
            daily_page_count: 0,
            due_key: DUE_KEY.to_owned(),
            generation: 1_795_394_100_000,
            local_date: "2026-11-23".to_owned(),
        })
        .expect("state")
    }

    /// construct one typed projection fixture.
    fn projection(value: u8, channel: ForecastCycleProjectionChannel) -> ForecastCycleProjection {
        ForecastCycleProjection {
            channel,
            identity_sha256: digest(value),
        }
    }

    // verify immutable append ordering, predecessor binding, offsets, and two slots
    #[test]
    fn appends_sorted_pages_into_current_and_next_slots() {
        let original = state();
        let codec = FixtureCodec;
        let mut persistence = PagePersistence::default();
        let first_projections = [
            projection(3, ForecastCycleProjectionChannel::Organic),
            projection(2, ForecastCycleProjectionChannel::SchedulerRequest),
        ];
        let first = append_forecast_cycle_page(
            &original,
            ForecastCyclePageAppendInput {
                payload: b"first",
                projections: &first_projections,
            },
            &codec,
            &mut persistence,
        )
        .expect("first append");
        assert!(original.pages.is_empty());
        assert_eq!(first.pages.len(), 1);
        assert_eq!(
            first.pages[0].projection_identities,
            vec![digest(2), digest(3)]
        );
        assert_eq!(first.slots[0].slot, ForecastCyclePageSlotKind::Current);
        assert_eq!(first.organic_projection_count, 1);
        assert_eq!(first.scheduler_projection_count, 1);

        let second_projection = [projection(4, ForecastCycleProjectionChannel::Organic)];
        let second = append_forecast_cycle_page(
            &first,
            ForecastCyclePageAppendInput {
                payload: b"second",
                projections: &second_projection,
            },
            &codec,
            &mut persistence,
        )
        .expect("second append");
        assert_eq!(
            second.pages[1].predecessor_page_sha256,
            Some(first.pages[0].page_sha256)
        );
        assert_eq!(second.pages[1].payload_offset, 5);
        assert_eq!(second.slots[1].slot, ForecastCyclePageSlotKind::Next);
        assert_eq!(
            persistence.pages[0].slot,
            ForecastCyclePageSlotKind::Current
        );
        assert_eq!(
            persistence.pages[0].projections[0].identity_sha256,
            digest(2)
        );
        assert_eq!(persistence.pages[1].header.payload_offset, 5);
        assert_eq!(persistence.pages[1].payload, b"second");
        assert_eq!(
            persistence.pages[1].page_sha256,
            second.pages[1].page_sha256
        );

        assert_eq!(
            append_forecast_cycle_page(
                &second,
                ForecastCyclePageAppendInput {
                    payload: b"third",
                    projections: &[],
                },
                &codec,
                &mut persistence,
            ),
            Err(ForecastCyclePageError::Refused(
                ForecastCyclePageRefusal::SlotUnavailable
            ))
        );
    }

    // verify oldest-first acknowledgement, next promotion, and exact idempotency
    #[test]
    fn acknowledges_in_order_and_promotes_only_durable_next() {
        let codec = FixtureCodec;
        let mut page_persistence = PagePersistence::default();
        let first_projection = [projection(1, ForecastCycleProjectionChannel::Organic)];
        let first = append_forecast_cycle_page(
            &state(),
            ForecastCyclePageAppendInput {
                payload: b"first",
                projections: &first_projection,
            },
            &codec,
            &mut page_persistence,
        )
        .expect("first");
        let second_projection = [projection(
            2,
            ForecastCycleProjectionChannel::SchedulerRequest,
        )];
        let pending = append_forecast_cycle_page(
            &first,
            ForecastCyclePageAppendInput {
                payload: b"second",
                projections: &second_projection,
            },
            &codec,
            &mut page_persistence,
        )
        .expect("second");
        let first_input = ForecastCyclePageAckInput {
            acknowledged_at: ACK_ONE.to_owned(),
            page_sha256: pending.pages[0].page_sha256,
        };
        let mut persistence = AckPersistence::default();
        let acknowledged =
            ack_forecast_cycle_page(&pending, &first_input, &codec, &mut persistence)
                .expect("first acknowledgement");
        assert_eq!(acknowledged.acknowledgements.len(), 1);
        assert_eq!(acknowledged.slots.len(), 1);
        assert_eq!(acknowledged.slots[0].page_index, 1);
        assert_eq!(
            acknowledged.slots[0].slot,
            ForecastCyclePageSlotKind::Current
        );
        assert_eq!(
            persistence.acknowledgements[0].next_slots,
            acknowledged.slots
        );
        assert_eq!(
            persistence.acknowledgements[0].acknowledgement,
            acknowledged.acknowledgements[0].acknowledgement
        );
        assert_eq!(
            persistence.acknowledgements[0].acknowledgement_sha256,
            acknowledged.acknowledgements[0].acknowledgement_sha256
        );

        let retry = ack_forecast_cycle_page(&acknowledged, &first_input, &codec, &mut persistence)
            .expect("idempotent retry");
        assert_eq!(retry, acknowledged);
        assert_eq!(persistence.acknowledgements.len(), 1);

        let collision = ForecastCyclePageAckInput {
            acknowledged_at: ACK_TWO.to_owned(),
            ..first_input
        };
        assert_eq!(
            ack_forecast_cycle_page(&acknowledged, &collision, &codec, &mut persistence),
            Err(ForecastCyclePageError::Refused(
                ForecastCyclePageRefusal::AcknowledgementCollision
            ))
        );
        let completed = ack_forecast_cycle_page(
            &acknowledged,
            &ForecastCyclePageAckInput {
                acknowledged_at: ACK_TWO.to_owned(),
                page_sha256: acknowledged.pages[1].page_sha256,
            },
            &codec,
            &mut persistence,
        )
        .expect("second acknowledgement");
        assert!(completed.slots.is_empty());
        assert_eq!(completed.acknowledgements.len(), 2);
    }

    // verify projection admission classes and exact durability proof binding
    #[test]
    fn refuses_duplicates_limits_and_mismatched_fsync_proofs() {
        let state = state();
        let codec = FixtureCodec;
        let duplicate = [
            projection(1, ForecastCycleProjectionChannel::Organic),
            projection(1, ForecastCycleProjectionChannel::SchedulerRequest),
        ];
        let mut persistence = PagePersistence::default();
        assert_eq!(
            append_forecast_cycle_page(
                &state,
                ForecastCyclePageAppendInput {
                    payload: b"payload",
                    projections: &duplicate,
                },
                &codec,
                &mut persistence,
            ),
            Err(ForecastCyclePageError::InvalidInput)
        );
        let scheduler = [
            projection(1, ForecastCycleProjectionChannel::SchedulerRequest),
            projection(2, ForecastCycleProjectionChannel::SchedulerRequest),
            projection(3, ForecastCycleProjectionChannel::SchedulerRequest),
            projection(4, ForecastCycleProjectionChannel::SchedulerRequest),
            projection(5, ForecastCycleProjectionChannel::SchedulerRequest),
        ];
        assert_eq!(
            append_forecast_cycle_page(
                &state,
                ForecastCyclePageAppendInput {
                    payload: b"payload",
                    projections: &scheduler,
                },
                &codec,
                &mut persistence,
            ),
            Err(ForecastCyclePageError::Refused(
                ForecastCyclePageRefusal::ProjectionLimit
            ))
        );

        persistence.override_proof = Some(ForecastCyclePageDurabilityProof {
            fsynced: true,
            page_sha256: digest(99),
        });
        assert_eq!(
            append_forecast_cycle_page(
                &state,
                ForecastCyclePageAppendInput {
                    payload: b"payload",
                    projections: &[],
                },
                &codec,
                &mut persistence,
            ),
            Err(ForecastCyclePageError::Refused(
                ForecastCyclePageRefusal::DurabilityProofRefused
            ))
        );
        assert!(state.pages.is_empty());
    }
}
