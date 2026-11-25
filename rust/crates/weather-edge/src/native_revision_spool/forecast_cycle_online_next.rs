//! select one final, page, or idle archive transfer under pre-held native custody.

use weather_contracts::ArchiveTransferIdleEnvelopeV1ContractVersionLiteral;

use std::fs;
use std::path::Path;

use crate::forecast_cycle_pages::{
    ForecastCyclePageHeader, ForecastCyclePageSlotKind, ForecastCycleProjection,
    ForecastCycleSha256,
};
use crate::native_revision_spool_file::sync_directory;

use super::forecast_cycle_disk_slots::{
    ForecastCycleDiskSlotCodec, ForecastCycleDiskSlotError, ForecastCycleDiskSlotPage,
    NativeForecastCycleDiskSlots,
};
use super::forecast_cycle_transfer_checkpoint::{
    ForecastCycleTransferCheckpointCodec, ForecastCycleTransferCheckpointError,
    NativeForecastCycleTransferCheckpoints,
};

/// exact transfer envelope contract shared by final, page, and idle selections.
pub const FORECAST_ARCHIVE_TRANSFER_CONTRACT_VERSION: &str =
    ArchiveTransferIdleEnvelopeV1ContractVersionLiteral::WIRE_VALUE;

/// one validated oldest final whose transfer acknowledgement is not yet durable.
#[derive(Debug, PartialEq, Eq)]
pub struct ForecastCyclePendingFinal<Manifest> {
    pub manifest: Manifest,
    pub manifest_sha256: ForecastCycleSha256,
}

/// validated final recovery and pending-order selection below the same admitted root.
pub trait ForecastCyclePendingFinalProvider {
    type Manifest;
    type Error;

    /// borrow the evidence root bound to every returned final record.
    fn admitted_root(&self) -> &Path;

    /// finish any crash-recoverable final before inspecting pending transfer order.
    fn recover_acknowledged_finals(&mut self) -> Result<(), Self::Error>;

    /// return only the oldest validated final without a durable transfer acknowledgement.
    fn next_pending_final_manifest(
        &mut self,
    ) -> Result<Option<ForecastCyclePendingFinal<Self::Manifest>>, Self::Error>;
}

/// one typed transfer selected without exposing mutable storage internals.
#[derive(Debug, PartialEq, Eq)]
pub enum ForecastCycleOnlineNext<Manifest> {
    Final {
        contract_version: &'static str,
        manifest: Manifest,
        manifest_sha256: ForecastCycleSha256,
    },
    Page {
        contract_version: &'static str,
        header: ForecastCyclePageHeader,
        page_sha256: ForecastCycleSha256,
        projections: Vec<ForecastCycleProjection>,
        payload: Vec<u8>,
    },
    Idle {
        contract_version: &'static str,
    },
}

/// closed native selection and crash-promotion failures.
#[derive(Debug)]
pub enum ForecastCycleOnlineNextError<FinalError> {
    RootMismatch,
    Final(FinalError),
    Current(ForecastCycleCurrentRecoveryError),
}

/// closed slot and predecessor-checkpoint recovery failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCycleCurrentRecoveryError {
    RootMismatch,
    Slot(ForecastCycleDiskSlotError),
    Transfer(ForecastCycleTransferCheckpointError),
    OrphanNext,
    PromotionStorage,
}

/// select one transfer in exact final-before-page-before-idle source order.
///
/// the caller must already hold the archive transport lease for every supplied native owner.
pub fn next_forecast_cycle_online<SlotCodec, SlotStatfs, TransferCodec, TransferStatfs, Final>(
    slots: &NativeForecastCycleDiskSlots<SlotCodec, SlotStatfs>,
    transfers: &NativeForecastCycleTransferCheckpoints<TransferCodec, TransferStatfs>,
    finals: &mut Final,
) -> Result<ForecastCycleOnlineNext<Final::Manifest>, ForecastCycleOnlineNextError<Final::Error>>
where
    SlotCodec: ForecastCycleDiskSlotCodec,
    TransferCodec: ForecastCycleTransferCheckpointCodec,
    Final: ForecastCyclePendingFinalProvider,
{
    use ForecastCycleOnlineNextError as Error;

    // bind every selection and promotion witness to one admitted custody root
    if slots.root() != transfers.root() || slots.root() != finals.admitted_root() {
        return Err(Error::RootMismatch);
    }
    finals.recover_acknowledged_finals().map_err(Error::Final)?;
    let pending = finals.next_pending_final_manifest().map_err(Error::Final)?;

    // transfer a genuine immutable closure before disclosing a later online page
    if let Some(pending) = pending {
        return Ok(ForecastCycleOnlineNext::Final {
            contract_version: FORECAST_ARCHIVE_TRANSFER_CONTRACT_VERSION,
            manifest: pending.manifest,
            manifest_sha256: pending.manifest_sha256,
        });
    }
    let current = recover_current_forecast_cycle_slot(slots, transfers).map_err(Error::Current)?;

    // expose only the stable idle marker after final and page selection are both empty
    let Some(current) = current else {
        return Ok(ForecastCycleOnlineNext::Idle {
            contract_version: FORECAST_ARCHIVE_TRANSFER_CONTRACT_VERSION,
        });
    };
    Ok(page_envelope(current))
}

/// recover a crash-left next slot only after its predecessor checkpoint is validated.
pub fn recover_current_forecast_cycle_slot<SlotCodec, SlotStatfs, TransferCodec, TransferStatfs>(
    slots: &NativeForecastCycleDiskSlots<SlotCodec, SlotStatfs>,
    transfers: &NativeForecastCycleTransferCheckpoints<TransferCodec, TransferStatfs>,
) -> Result<Option<ForecastCycleDiskSlotPage>, ForecastCycleCurrentRecoveryError>
where
    SlotCodec: ForecastCycleDiskSlotCodec,
    TransferCodec: ForecastCycleTransferCheckpointCodec,
{
    use ForecastCycleCurrentRecoveryError as Error;

    // refuse a checkpoint witness from any other evidence root
    if slots.root() != transfers.root() {
        return Err(Error::RootMismatch);
    }
    let current = slots
        .read_slot(ForecastCyclePageSlotKind::Current)
        .map_err(Error::Slot)?;

    // retain an already published current page without filesystem mutation
    if current.is_some() {
        return Ok(current);
    }
    let next = slots
        .read_slot(ForecastCyclePageSlotKind::Next)
        .map_err(Error::Slot)?;

    // retain the stable empty state when no crash promotion remains
    let Some(next) = next else {
        return Ok(None);
    };
    let predecessor = next
        .header
        .predecessor_page_sha256
        .ok_or(Error::OrphanNext)?;
    let checkpoint = transfers
        .read_transfer_acknowledgement(predecessor)
        .map_err(Error::Transfer)?;

    // promote only when exact canonical checkpoint custody proves the predecessor
    if checkpoint.is_none() {
        return Err(Error::OrphanNext);
    }
    promote_next_slot(slots.root()).map_err(|_| Error::PromotionStorage)?;
    slots
        .read_slot(ForecastCyclePageSlotKind::Current)
        .map_err(Error::Slot)
}

/// move one proven next slot into the empty current name and sync its fixed directory.
fn promote_next_slot(root: &Path) -> std::io::Result<()> {
    let directory = root.join("online-pages").join("slots");
    fs::rename(directory.join("next.page"), directory.join("current.page"))?;
    sync_directory(&directory)
}

/// move one validated native page into its transport-neutral typed envelope.
fn page_envelope<Manifest>(page: ForecastCycleDiskSlotPage) -> ForecastCycleOnlineNext<Manifest> {
    ForecastCycleOnlineNext::Page {
        contract_version: FORECAST_ARCHIVE_TRANSFER_CONTRACT_VERSION,
        header: page.header,
        page_sha256: page.page_sha256,
        projections: page.projections,
        payload: page.payload,
    }
}
