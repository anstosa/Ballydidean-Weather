//! Recover an open archival cycle from real transfer receipts and current/next files.

use crate::forecast_cycle_pages::{
    ForecastCyclePage, ForecastCyclePageCodec, ForecastCyclePageSlotKind,
    ForecastCyclePageStateInput, create_forecast_cycle_page_state, valid_due_key,
};
use crate::forecast_cycle_reconcile::{
    ForecastCycleDurableState, ForecastCycleReconcileError, ForecastCycleReconcileResult,
    reconcile_forecast_cycle_pages,
};
use crate::revision_archive_canonical::instant;

use super::forecast_allocation::ForecastAllocationStatfsPort;
use super::forecast_cycle_disk_ack::{ForecastCycleDiskAckError, ForecastCycleDiskAckReadCodec};
use super::forecast_cycle_disk_slots::{
    ForecastCycleDiskSlotCodec, ForecastCycleDiskSlotError, NativeForecastCycleDiskSlots,
};
use super::forecast_cycle_gap::{
    DurableForecastCycleGap, ForecastCycleGapCodec, ForecastCycleGapError,
};
use super::forecast_cycle_transfer_checkpoint::{
    ForecastCycleTransferCheckpoint, ForecastCycleTransferCheckpointCodec,
    ForecastCycleTransferCheckpointError, NativeForecastCycleTransferCheckpoints,
};

/// Reconciled open archive frontier and its separately verified durable gap records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeForecastCycleRecovery {
    pub cycle: ForecastCycleReconcileResult<(), ()>,
    pub gaps: Vec<DurableForecastCycleGap>,
}

impl NativeForecastCycleRecovery {
    /// Expose the open state with only the independently validated native gap custody.
    pub fn into_page_state(
        self,
    ) -> crate::forecast_cycle_pages::ForecastCyclePageState<DurableForecastCycleGap> {
        self.cycle.state.with_validated_gaps(self.gaps)
    }
}

/// Closed failure categories preserve incomplete checkpoint custody rather than resetting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeForecastCycleRecoveryError {
    RootMismatch,
    InvalidDueKey,
    OrphanNext,
    SlotCollision,
    TransferGenerationCollision,
    IncompleteAcknowledgement,
    Slot(ForecastCycleDiskSlotError),
    Checkpoint(ForecastCycleTransferCheckpointError),
    Acknowledgement(ForecastCycleDiskAckError),
    Gap(ForecastCycleGapError),
    Reconcile(ForecastCycleReconcileError),
}

/// Reopen only the admitted due key from physical durable holdings under the archive lease.
pub fn recover_native_forecast_cycle<SlotCodec, SlotStatfs, TransferCodec, TransferStatfs>(
    due_key: &str,
    slots: &NativeForecastCycleDiskSlots<SlotCodec, SlotStatfs>,
    transfers: &NativeForecastCycleTransferCheckpoints<TransferCodec, TransferStatfs>,
    codec: &impl ForecastCyclePageCodec,
) -> Result<NativeForecastCycleRecovery, NativeForecastCycleRecoveryError>
where
    SlotCodec: ForecastCycleDiskSlotCodec + ForecastCycleDiskAckReadCodec + ForecastCycleGapCodec,
    SlotStatfs: ForecastAllocationStatfsPort,
    TransferCodec: ForecastCycleTransferCheckpointCodec,
{
    use NativeForecastCycleRecoveryError as Error;

    // keep all recovery reads bound to the same admitted evidence root
    if slots.root() != transfers.root() {
        return Err(Error::RootMismatch);
    }
    // check the existing finite-cycle key before using its fixed wire positions
    if !valid_due_key(due_key) {
        return Err(Error::InvalidDueKey);
    }
    let current = slots
        .read_slot(ForecastCyclePageSlotKind::Current)
        .map_err(Error::Slot)?;
    let mut checkpoints = transfers
        .list_transfer_acknowledgements()
        .map_err(Error::Checkpoint)?;
    checkpoints.retain(|checkpoint| checkpoint.header.due_key == due_key);
    checkpoints.sort_by_key(|checkpoint| checkpoint.header.page_index);
    let generation = current
        .as_ref()
        .map(|page| page.header.generation)
        .or_else(|| checkpoints.first().map(|value| value.header.generation))
        .or_else(|| u64::try_from(instant(codec, &due_key[8..])?).ok())
        .ok_or(Error::InvalidDueKey)?;
    let daily_page_count = due_key[19..21]
        .parse::<usize>()
        .map_err(|_| Error::InvalidDueKey)?
        / 6;
    let expected = create_forecast_cycle_page_state(ForecastCyclePageStateInput {
        daily_page_count,
        due_key: due_key.to_owned(),
        generation,
        local_date: due_key[8..18].to_owned(),
    })
    .ok_or(Error::InvalidDueKey)?;

    let next = slots
        .read_slot(ForecastCyclePageSlotKind::Next)
        .map_err(Error::Slot)?;
    // do not start a new frontier over a crash-left orphan next slot
    if current.is_none() && next.is_some() {
        return Err(Error::OrphanNext);
    }
    let mut pages = Vec::with_capacity(checkpoints.len() + 2);
    let mut acknowledgements = Vec::with_capacity(checkpoints.len());
    // restore acknowledged page footprints without reproducing deleted payload bytes
    for checkpoint in checkpoints {
        if checkpoint.header.generation != generation {
            return Err(Error::TransferGenerationCollision);
        }
        let ack = slots
            .read_cycle_page_acknowledgement(checkpoint.page_sha256)
            .map_err(Error::Acknowledgement)?
            .ok_or(Error::IncompleteAcknowledgement)?;
        acknowledgements.push(ack);
        pages.push(page_metadata(checkpoint));
    }
    // append only pending slots not represented in the immutable acknowledgement prefix
    for slot in [current, next].into_iter().flatten() {
        if pages
            .iter()
            .any(|page| page.page_sha256 == slot.page_sha256)
        {
            continue;
        }
        if slot.header.due_key != due_key || slot.header.generation != generation {
            return Err(Error::SlotCollision);
        }
        pages.push(slot.page_metadata());
    }
    let gaps = slots
        .read_cycle_gaps(due_key, generation)
        .map_err(Error::Gap)?;
    let cycle = reconcile_forecast_cycle_pages(
        &expected,
        ForecastCycleDurableState::open(pages, acknowledgements),
        codec,
    )
    .map_err(Error::Reconcile)?;
    Ok(NativeForecastCycleRecovery { cycle, gaps })
}

/// Project only independently validated checkpoint metadata into the retained frontier.
pub(super) fn page_metadata(checkpoint: ForecastCycleTransferCheckpoint) -> ForecastCyclePage {
    ForecastCyclePage {
        page_index: checkpoint.header.page_index,
        page_sha256: checkpoint.page_sha256,
        predecessor_page_sha256: checkpoint.header.predecessor_page_sha256,
        payload_sha256: checkpoint.header.payload_sha256,
        payload_offset: checkpoint.header.payload_offset,
        payload_length: checkpoint.payload_length,
        projection_identities: checkpoint
            .projections
            .iter()
            .map(|projection| projection.identity_sha256)
            .collect(),
        projection_channels: checkpoint
            .projections
            .iter()
            .map(|projection| projection.channel)
            .collect(),
    }
}
