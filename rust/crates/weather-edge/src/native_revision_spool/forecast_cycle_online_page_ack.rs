//! acknowledge one online page through native checkpoint and slot durability owners.

use std::cell::RefCell;

use crate::forecast_cycle_pages::{
    FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION, ForecastCyclePageAckPersistence,
    ForecastCyclePageAckPersistenceRequest, ForecastCyclePageAcknowledgement,
    ForecastCyclePageCodec, ForecastCyclePageSlot, ForecastCyclePageSlotKind, ForecastCycleSha256,
};
use crate::revision_archive_canonical::RevisionArchiveCanonicalClock;

use super::forecast_allocation::ForecastAllocationStatfsPort;
use super::forecast_archive_lock::{
    ForecastArchiveLockCodec, NativeForecastArchiveLeaseError, with_native_forecast_archive_lease,
};
use super::forecast_archive_lock_binding::BoundForecastArchiveLockClock;
use super::forecast_cycle_disk_ack::{
    ForecastCycleDiskAckError, NativeForecastCycleDiskAcknowledgements,
};
use super::forecast_cycle_disk_slots::{
    ForecastCycleDiskSlotCodec, ForecastCycleDiskSlotError, NativeForecastCycleDiskSlots,
};
use super::forecast_cycle_online_next::{
    ForecastCycleCurrentRecoveryError, recover_current_forecast_cycle_slot,
};
use super::forecast_cycle_transfer_checkpoint::{
    FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION, ForecastCycleTransferCheckpoint,
    ForecastCycleTransferCheckpointCodec, ForecastCycleTransferCheckpointError,
    NativeForecastCycleTransferCheckpoints,
};

/// closed acknowledgement failures preserve retry, custody, and durability distinctions.
#[derive(Debug)]
pub enum ForecastCycleOnlinePageAckError<FinalError> {
    RootMismatch,
    LeaseBusy,
    LeaseStorage,
    CheckpointCollision,
    PageNotPending,
    ClockUnavailable,
    Codec,
    DurabilityProofRefused,
    Slot(ForecastCycleDiskSlotError),
    Transfer(ForecastCycleTransferCheckpointError),
    Acknowledgement(ForecastCycleDiskAckError),
    Recovery(ForecastCycleCurrentRecoveryError),
    Final(FinalError),
}

/// one admitted page and workstation checkpoint identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastCycleOnlinePageAckRequest {
    pub page_sha256: ForecastCycleSha256,
    pub checkpoint_sha256: ForecastCycleSha256,
}

/// borrowed concrete native owners and the single canonical clock for one acknowledgement.
pub struct ForecastCycleOnlinePageAckPorts<
    'a,
    SlotCodec,
    SlotStatfs,
    AckCodec,
    AckStatfs,
    TransferCodec,
    TransferStatfs,
    DigestCodec,
    Clock,
    LockCodec,
> {
    acknowledgements: &'a mut NativeForecastCycleDiskAcknowledgements<
        NativeForecastCycleDiskSlots<SlotCodec, SlotStatfs>,
        AckCodec,
        AckStatfs,
    >,
    transfers: &'a mut NativeForecastCycleTransferCheckpoints<TransferCodec, TransferStatfs>,
    digest_codec: &'a DigestCodec,
    clock: &'a mut Clock,
    lock_codec: &'a LockCodec,
}

impl<
    'a,
    SlotCodec,
    SlotStatfs,
    AckCodec,
    AckStatfs,
    TransferCodec,
    TransferStatfs,
    DigestCodec,
    Clock,
    LockCodec,
>
    ForecastCycleOnlinePageAckPorts<
        'a,
        SlotCodec,
        SlotStatfs,
        AckCodec,
        AckStatfs,
        TransferCodec,
        TransferStatfs,
        DigestCodec,
        Clock,
        LockCodec,
    >
{
    /// bind the existing writers, codec, and clock without creating another storage owner.
    pub fn new(
        acknowledgements: &'a mut NativeForecastCycleDiskAcknowledgements<
            NativeForecastCycleDiskSlots<SlotCodec, SlotStatfs>,
            AckCodec,
            AckStatfs,
        >,
        transfers: &'a mut NativeForecastCycleTransferCheckpoints<TransferCodec, TransferStatfs>,
        digest_codec: &'a DigestCodec,
        clock: &'a mut Clock,
        lock_codec: &'a LockCodec,
    ) -> Self {
        Self {
            acknowledgements,
            transfers,
            digest_codec,
            clock,
            lock_codec,
        }
    }
}

/// acknowledge one page under the real archive lease and recover later durable work.
pub fn acknowledge_forecast_cycle_online_page<
    SlotCodec,
    SlotStatfs,
    AckCodec,
    AckStatfs,
    TransferCodec,
    TransferStatfs,
    DigestCodec,
    Clock,
    LockCodec,
    RecoverFinals,
    FinalError,
>(
    request: ForecastCycleOnlinePageAckRequest,
    ports: ForecastCycleOnlinePageAckPorts<
        '_,
        SlotCodec,
        SlotStatfs,
        AckCodec,
        AckStatfs,
        TransferCodec,
        TransferStatfs,
        DigestCodec,
        Clock,
        LockCodec,
    >,
    recover_acknowledged_finals: RecoverFinals,
) -> Result<ForecastCycleTransferCheckpoint, ForecastCycleOnlinePageAckError<FinalError>>
where
    SlotCodec: ForecastCycleDiskSlotCodec,
    AckCodec: ForecastCyclePageCodec,
    AckStatfs: ForecastAllocationStatfsPort,
    TransferCodec: ForecastCycleTransferCheckpointCodec,
    TransferStatfs: ForecastAllocationStatfsPort,
    DigestCodec: ForecastCyclePageCodec,
    Clock: RevisionArchiveCanonicalClock,
    LockCodec: ForecastArchiveLockCodec,
    RecoverFinals: FnOnce(
        &NativeForecastCycleDiskSlots<SlotCodec, SlotStatfs>,
        &NativeForecastCycleTransferCheckpoints<TransferCodec, TransferStatfs>,
    ) -> Result<(), FinalError>,
{
    use ForecastCycleOnlinePageAckError as Error;

    let ForecastCycleOnlinePageAckPorts {
        acknowledgements,
        transfers,
        digest_codec,
        clock,
        lock_codec,
    } = ports;
    let root = acknowledgements.slots().root().to_path_buf();
    // refuse receipts from a different archive before taking either storage owner mutable
    if root != transfers.root() {
        return Err(Error::RootMismatch);
    }
    let clock = RefCell::new(clock);
    let mut lease_clock = SharedCanonicalClock(&clock);
    let mut lease_clock = BoundForecastArchiveLockClock::new(&mut lease_clock, digest_codec);
    match with_native_forecast_archive_lease(&root, lock_codec, &mut lease_clock, || {
        acknowledge_under_custody(
            acknowledgements,
            transfers,
            request,
            digest_codec,
            // sample the same clock only when a first transfer receipt is actually needed
            || clock.try_borrow_mut().ok()?.canonical_now(),
            recover_acknowledged_finals,
        )
    }) {
        Ok(checkpoint) => Ok(checkpoint),
        Err(NativeForecastArchiveLeaseError::Operation(error)) => Err(error),
        Err(NativeForecastArchiveLeaseError::Busy) => Err(Error::LeaseBusy),
        Err(NativeForecastArchiveLeaseError::Storage(_)) => Err(Error::LeaseStorage),
    }
}

/// preserve first-writer checkpoint identity before releasing or promoting physical slots.
fn acknowledge_under_custody<
    SlotCodec,
    SlotStatfs,
    AckCodec,
    AckStatfs,
    TransferCodec,
    TransferStatfs,
    DigestCodec,
    SampleClock,
    RecoverFinals,
    FinalError,
>(
    acknowledgements: &mut NativeForecastCycleDiskAcknowledgements<
        NativeForecastCycleDiskSlots<SlotCodec, SlotStatfs>,
        AckCodec,
        AckStatfs,
    >,
    transfers: &mut NativeForecastCycleTransferCheckpoints<TransferCodec, TransferStatfs>,
    request: ForecastCycleOnlinePageAckRequest,
    digest_codec: &DigestCodec,
    sample_clock: SampleClock,
    recover_acknowledged_finals: RecoverFinals,
) -> Result<ForecastCycleTransferCheckpoint, ForecastCycleOnlinePageAckError<FinalError>>
where
    SlotCodec: ForecastCycleDiskSlotCodec,
    AckCodec: ForecastCyclePageCodec,
    AckStatfs: ForecastAllocationStatfsPort,
    TransferCodec: ForecastCycleTransferCheckpointCodec,
    TransferStatfs: ForecastAllocationStatfsPort,
    DigestCodec: ForecastCyclePageCodec,
    SampleClock: FnOnce() -> Option<String>,
    RecoverFinals: FnOnce(
        &NativeForecastCycleDiskSlots<SlotCodec, SlotStatfs>,
        &NativeForecastCycleTransferCheckpoints<TransferCodec, TransferStatfs>,
    ) -> Result<(), FinalError>,
{
    use ForecastCycleOnlinePageAckError as Error;

    let ForecastCycleOnlinePageAckRequest {
        page_sha256,
        checkpoint_sha256,
    } = request;
    let existing = transfers
        .read_transfer_acknowledgement(page_sha256)
        .map_err(Error::Transfer)?;
    // reuse the original first receipt or create it only from the exact active page
    let checkpoint = if let Some(existing) = existing {
        // preserve the first workstation graph binding on exact retries only
        if existing.checkpoint_sha256 != checkpoint_sha256 {
            return Err(Error::CheckpointCollision);
        }
        existing
    } else {
        let current = acknowledgements
            .slots()
            .read_slot(ForecastCyclePageSlotKind::Current)
            .map_err(Error::Slot)?;
        let Some(current) = current.filter(|page| page.page_sha256 == page_sha256) else {
            return Err(Error::PageNotPending);
        };
        let acknowledged_at = sample_clock().ok_or(Error::ClockUnavailable)?;
        let checkpoint = ForecastCycleTransferCheckpoint {
            acknowledged_at,
            checkpoint_sha256,
            contract_version: FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION,
            header: current.header,
            page_sha256,
            payload_length: current.payload.len(),
            projections: current.projections,
        };
        let proof = transfers
            .persist_transfer_acknowledgement(&checkpoint)
            .map_err(Error::Transfer)?;

        // expose no checkpoint unless the writer confirms the requested page identity
        if !proof.fsynced || proof.page_sha256 != page_sha256 {
            return Err(Error::DurabilityProofRefused);
        }
        checkpoint
    };
    let current = acknowledgements
        .slots()
        .read_slot(ForecastCyclePageSlotKind::Current)
        .map_err(Error::Slot)?;

    // finish physical page acknowledgement only while the original page remains current
    if current
        .as_ref()
        .is_some_and(|page| page.page_sha256 == page_sha256)
    {
        let current = current.as_ref().ok_or(Error::PageNotPending)?;
        let page_acknowledgement = ForecastCyclePageAcknowledgement {
            contract_version: FORECAST_CYCLE_PAGE_ACK_CONTRACT_VERSION,
            due_key: current.header.due_key.clone(),
            generation: current.header.generation,
            page_index: current.header.page_index,
            page_sha256,
            acknowledged_at: checkpoint.acknowledged_at.clone(),
        };
        let bytes = digest_codec
            .encode_page_acknowledgement(&page_acknowledgement)
            .ok_or(Error::Codec)?;
        let acknowledgement_sha256 = ForecastCycleSha256::admitted(digest_codec.sha256(&bytes));
        let next = acknowledgements
            .slots()
            .read_slot(ForecastCyclePageSlotKind::Next)
            .map_err(Error::Slot)?;
        let next_slots = next
            .map(|page| ForecastCyclePageSlot {
                page_index: page.header.page_index,
                page_sha256: page.page_sha256,
                slot: ForecastCyclePageSlotKind::Next,
            })
            .into_iter()
            .collect::<Vec<_>>();
        let proof = acknowledgements
            .persist_acknowledgement(ForecastCyclePageAckPersistenceRequest {
                acknowledgement: &page_acknowledgement,
                acknowledgement_sha256,
                next_slots: &next_slots,
            })
            .map_err(Error::Acknowledgement)?;

        // require the exact fsynced page-protocol receipt before final recovery
        if !proof.fsynced || proof.acknowledgement_sha256 != acknowledgement_sha256 {
            return Err(Error::DurabilityProofRefused);
        }
    } else if current.is_none() {
        // finish a crash-interrupted next promotion using the durable transfer checkpoint
        recover_current_forecast_cycle_slot(acknowledgements.slots(), transfers)
            .map_err(Error::Recovery)?;
    }
    recover_acknowledged_finals(acknowledgements.slots(), transfers).map_err(Error::Final)?;
    Ok(checkpoint)
}

/// reborrow one injected clock without holding it across the guarded operation.
struct SharedCanonicalClock<'a, 'clock, Clock>(&'a RefCell<&'clock mut Clock>);

impl<Clock: RevisionArchiveCanonicalClock> RevisionArchiveCanonicalClock
    for SharedCanonicalClock<'_, '_, Clock>
{
    /// observe one canonical instant for lease or first-receipt chronology.
    fn canonical_now(&mut self) -> Option<String> {
        self.0.try_borrow_mut().ok()?.canonical_now()
    }
}
