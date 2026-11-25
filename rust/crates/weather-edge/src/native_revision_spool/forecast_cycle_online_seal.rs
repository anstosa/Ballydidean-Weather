//! seal one fully acknowledged forecast cycle under the native archive lease.

use std::cell::RefCell;

use crate::ForecastEvidenceReceiptCodec;
use crate::forecast_cycle_pages::{
    ForecastCyclePageSlotKind, ForecastCyclePageStateInput, create_forecast_cycle_page_state,
};
use crate::forecast_cycle_reconcile::{
    ForecastCycleDurableState, ForecastCycleReconcileError, reconcile_forecast_cycle_pages,
};
use crate::revision_archive_canonical::RevisionArchiveCanonicalClock;

use super::forecast_allocation::ForecastAllocationStatfsPort;
use super::forecast_archive_lock::{
    ForecastArchiveLockCodec, NativeForecastArchiveLeaseError, with_native_forecast_archive_lease,
};
use super::forecast_archive_lock_binding::BoundForecastArchiveLockClock;
use super::forecast_cycle_disk_ack::ForecastCycleDiskAckError;
use super::forecast_cycle_disk_recovery::page_metadata;
use super::forecast_cycle_disk_slots::{ForecastCycleDiskSlotError, NativeForecastCycleDiskSlots};
use super::forecast_cycle_final_binding::{
    ForecastCycleFinalizationError, prepare_forecast_cycle_input_seal,
};
use super::forecast_cycle_gap::ForecastCycleGapError;
use super::forecast_cycle_input_seal::{
    ForecastCycleInputSeal, ForecastCycleInputSealError, ForecastCycleRequiredInputs,
    NativeForecastCycleInputSeals,
};
use super::forecast_cycle_page_binding::{BoundForecastCyclePageCodec, ForecastCycleHashPair};
use super::forecast_cycle_transfer_checkpoint::{
    ForecastCycleTransferCheckpointError, NativeForecastCycleTransferCheckpoints,
};

/// exact operator input after wire validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCycleOnlineSealRequest {
    pub due_key: String,
    pub generation: u64,
    pub required_inputs: ForecastCycleRequiredInputs,
}

/// closed seal failures preserve incomplete custody and immutable collision categories.
#[derive(Debug)]
pub enum ForecastCycleOnlineSealError<FinalError> {
    RootMismatch,
    LeaseBusy,
    LeaseStorage,
    InputsNotAcknowledged,
    NoAcknowledgedPages,
    IncompleteAcknowledgementChain,
    InvalidCycle,
    ClockUnavailable,
    DurabilityProofRefused,
    Slot(ForecastCycleDiskSlotError),
    Transfer(ForecastCycleTransferCheckpointError),
    Acknowledgement(ForecastCycleDiskAckError),
    Gap(ForecastCycleGapError),
    Seal(ForecastCycleInputSealError),
    Reconcile(ForecastCycleReconcileError),
    Prepare(ForecastCycleFinalizationError),
    Final(FinalError),
}

/// borrowed actual native owners and one canonical clock for a seal operation.
pub struct ForecastCycleOnlineSealPorts<'ports, 'codec, Codec, Statfs, Clock, LockCodec> {
    slots: &'ports NativeForecastCycleDiskSlots<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
    transfers: &'ports NativeForecastCycleTransferCheckpoints<
        BoundForecastCyclePageCodec<'codec, Codec>,
        Statfs,
    >,
    seals: &'ports mut NativeForecastCycleInputSeals<
        BoundForecastCyclePageCodec<'codec, Codec>,
        Statfs,
    >,
    codec: &'ports BoundForecastCyclePageCodec<'codec, Codec>,
    clock: &'ports mut Clock,
    lock_codec: &'ports LockCodec,
}

impl<'ports, 'codec, Codec, Statfs, Clock, LockCodec>
    ForecastCycleOnlineSealPorts<'ports, 'codec, Codec, Statfs, Clock, LockCodec>
{
    /// bind the existing native stores without creating another filesystem owner.
    pub fn new(
        slots: &'ports NativeForecastCycleDiskSlots<
            BoundForecastCyclePageCodec<'codec, Codec>,
            Statfs,
        >,
        transfers: &'ports NativeForecastCycleTransferCheckpoints<
            BoundForecastCyclePageCodec<'codec, Codec>,
            Statfs,
        >,
        seals: &'ports mut NativeForecastCycleInputSeals<
            BoundForecastCyclePageCodec<'codec, Codec>,
            Statfs,
        >,
        codec: &'ports BoundForecastCyclePageCodec<'codec, Codec>,
        clock: &'ports mut Clock,
        lock_codec: &'ports LockCodec,
    ) -> Self {
        Self {
            slots,
            transfers,
            seals,
            codec,
            clock,
            lock_codec,
        }
    }
}

/// persist the first exact producer seal, then recover any newly closable final.
pub fn seal_forecast_cycle_inputs<
    'codec,
    Codec,
    Statfs,
    Clock,
    LockCodec,
    RecoverFinals,
    FinalError,
>(
    request: ForecastCycleOnlineSealRequest,
    ports: ForecastCycleOnlineSealPorts<'_, 'codec, Codec, Statfs, Clock, LockCodec>,
    recover_acknowledged_finals: RecoverFinals,
) -> Result<ForecastCycleInputSeal, ForecastCycleOnlineSealError<FinalError>>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: ForecastAllocationStatfsPort,
    Clock: RevisionArchiveCanonicalClock,
    LockCodec: ForecastArchiveLockCodec,
    RecoverFinals: FnOnce(
        &NativeForecastCycleDiskSlots<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
        &NativeForecastCycleTransferCheckpoints<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
        &NativeForecastCycleInputSeals<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
    ) -> Result<(), FinalError>,
{
    use ForecastCycleOnlineSealError as Error;

    let ForecastCycleOnlineSealPorts {
        slots,
        transfers,
        seals,
        codec,
        clock,
        lock_codec,
    } = ports;
    let root = slots.root().to_path_buf();
    // bind every census and durable seal to one admitted archive root
    if root != transfers.root() || root != seals.root() {
        return Err(Error::RootMismatch);
    }
    let clock = RefCell::new(clock);
    let mut lease_clock = SharedCanonicalClock(&clock);
    let mut lease_clock = BoundForecastArchiveLockClock::new(&mut lease_clock, codec);
    match with_native_forecast_archive_lease(&root, lock_codec, &mut lease_clock, || {
        seal_under_custody(
            request,
            slots,
            transfers,
            seals,
            codec,
            // sample the same clock only when no first seal clock already exists
            || clock.try_borrow_mut().ok()?.canonical_now(),
            recover_acknowledged_finals,
        )
    }) {
        Ok(seal) => Ok(seal),
        Err(NativeForecastArchiveLeaseError::Operation(error)) => Err(error),
        Err(NativeForecastArchiveLeaseError::Busy) => Err(Error::LeaseBusy),
        Err(NativeForecastArchiveLeaseError::Storage(_)) => Err(Error::LeaseStorage),
    }
}

/// reconstruct one complete acknowledgement group before installing its first seal.
fn seal_under_custody<'codec, Codec, Statfs, SampleClock, RecoverFinals, FinalError>(
    request: ForecastCycleOnlineSealRequest,
    slots: &NativeForecastCycleDiskSlots<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
    transfers: &NativeForecastCycleTransferCheckpoints<
        BoundForecastCyclePageCodec<'codec, Codec>,
        Statfs,
    >,
    seals: &mut NativeForecastCycleInputSeals<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
    codec: &BoundForecastCyclePageCodec<'codec, Codec>,
    sample_clock: SampleClock,
    recover_acknowledged_finals: RecoverFinals,
) -> Result<ForecastCycleInputSeal, ForecastCycleOnlineSealError<FinalError>>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: ForecastAllocationStatfsPort,
    SampleClock: FnOnce() -> Option<String>,
    RecoverFinals: FnOnce(
        &NativeForecastCycleDiskSlots<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
        &NativeForecastCycleTransferCheckpoints<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
        &NativeForecastCycleInputSeals<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
    ) -> Result<(), FinalError>,
{
    use ForecastCycleOnlineSealError as Error;

    let current = slots
        .read_slot(ForecastCyclePageSlotKind::Current)
        .map_err(Error::Slot)?;
    let next = slots
        .read_slot(ForecastCyclePageSlotKind::Next)
        .map_err(Error::Slot)?;

    // never close a cycle while either physical slot retains one of its pages
    if [current.as_ref(), next.as_ref()]
        .into_iter()
        .flatten()
        .any(|page| {
            page.header.due_key == request.due_key && page.header.generation == request.generation
        })
    {
        return Err(Error::InputsNotAcknowledged);
    }
    let mut group = transfers
        .list_transfer_acknowledgements()
        .map_err(Error::Transfer)?
        .into_iter()
        .filter(|entry| {
            entry.header.due_key == request.due_key && entry.header.generation == request.generation
        })
        .collect::<Vec<_>>();
    group.sort_by_key(|entry| entry.header.page_index);

    // refuse a closure with no acknowledged page population
    if group.is_empty() {
        return Err(Error::NoAcknowledgedPages);
    }
    let mut pages = Vec::with_capacity(group.len());
    let mut acknowledgements = Vec::with_capacity(group.len());

    // bind each ordered workstation receipt to its durable page-protocol acknowledgement
    for (index, transfer) in group.into_iter().enumerate() {
        if transfer.header.page_index != index {
            return Err(Error::IncompleteAcknowledgementChain);
        }
        let acknowledgement = slots
            .read_cycle_page_acknowledgement(transfer.page_sha256)
            .map_err(Error::Acknowledgement)?
            .ok_or(Error::IncompleteAcknowledgementChain)?;
        pages.push(page_metadata(transfer));
        acknowledgements.push(acknowledgement);
    }
    let hour = request
        .due_key
        .get(19..21)
        .ok_or(Error::InvalidCycle)?
        .parse::<usize>()
        .map_err(|_| Error::InvalidCycle)?;
    let initial = create_forecast_cycle_page_state(ForecastCyclePageStateInput {
        daily_page_count: hour / 6,
        due_key: request.due_key.clone(),
        generation: request.generation,
        local_date: request
            .due_key
            .get(8..18)
            .ok_or(Error::InvalidCycle)?
            .to_owned(),
    })
    .ok_or(Error::InvalidCycle)?;
    let gaps = slots
        .read_cycle_gaps(&request.due_key, request.generation)
        .map_err(Error::Gap)?;
    let state = reconcile_forecast_cycle_pages(
        &initial,
        ForecastCycleDurableState::open(pages, acknowledgements),
        codec,
    )
    .map_err(Error::Reconcile)?
    .state
    .with_validated_gaps(gaps);
    let existing = seals
        .read_input_seal(&request.due_key, request.generation)
        .map_err(Error::Seal)?;
    let sealed_at = existing
        .as_ref()
        .map(|seal| seal.sealed_at.clone())
        .map_or_else(sample_clock, Some)
        .ok_or(Error::ClockUnavailable)?;
    let seal =
        prepare_forecast_cycle_input_seal(&state, request.required_inputs, &sealed_at, codec)
            .map_err(Error::Prepare)?;
    let proof = seals.persist_input_seal(&seal).map_err(Error::Seal)?;

    // recover finals only after the exact producer closure is durably acknowledged
    if !proof.fsynced {
        return Err(Error::DurabilityProofRefused);
    }
    recover_acknowledged_finals(slots, transfers, seals).map_err(Error::Final)?;
    Ok(seal)
}

/// reborrow one injected clock without holding it across the guarded operation.
struct SharedCanonicalClock<'a, 'clock, Clock>(&'a RefCell<&'clock mut Clock>);

impl<Clock: RevisionArchiveCanonicalClock> RevisionArchiveCanonicalClock
    for SharedCanonicalClock<'_, '_, Clock>
{
    /// observe one canonical instant for lease or first-seal chronology.
    fn canonical_now(&mut self) -> Option<String> {
        self.0.try_borrow_mut().ok()?.canonical_now()
    }
}
