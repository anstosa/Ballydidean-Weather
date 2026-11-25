//! Run the native final-before-page transfer and first cold-graph acknowledgement lanes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use crate::ForecastEvidenceReceiptCodec;
use crate::forecast_cycle_pages::{
    ForecastCyclePageSlotKind, ForecastCyclePageStateInput, ForecastCycleSha256,
    create_forecast_cycle_page_state,
};
use crate::forecast_cycle_reconcile::{
    ForecastCycleDurableState, ForecastCycleReconcileError, reconcile_forecast_cycle_pages,
};
use crate::revision_archive_canonical::RevisionArchiveCanonicalClock;

use super::forecast_allocation::ForecastAllocationStatfsPort;
use super::forecast_archive_lock::{
    ForecastArchiveLockClock, ForecastArchiveLockCodec, NativeForecastArchiveLeaseError,
    with_native_forecast_archive_lease,
};
use super::forecast_archive_lock_binding::BoundForecastArchiveLockClock;
use super::forecast_cycle_disk_ack::ForecastCycleDiskAckError;
use super::forecast_cycle_disk_recovery::page_metadata;
use super::forecast_cycle_disk_slots::{ForecastCycleDiskSlotError, NativeForecastCycleDiskSlots};
use super::forecast_cycle_final_ack::{
    FORECAST_CYCLE_FINAL_ACK_CONTRACT_VERSION, ForecastCycleFinalAckError,
    ForecastCycleFinalAcknowledgement, NativeForecastCycleFinalAcknowledgements,
};
use super::forecast_cycle_final_binding::{
    ForecastCycleFinalManifest, ForecastCycleFinalizationError,
    prepare_forecast_cycle_final_manifest,
};
use super::forecast_cycle_final_store::{
    ForecastCycleFinalManifestError, NativeForecastCycleFinalManifests,
};
use super::forecast_cycle_gap::ForecastCycleGapError;
use super::forecast_cycle_input_seal::{
    ForecastCycleInputSealError, NativeForecastCycleInputSeals,
};
use super::forecast_cycle_online_next::{
    ForecastCycleOnlineNext, ForecastCycleOnlineNextError, ForecastCyclePendingFinal,
    ForecastCyclePendingFinalProvider, next_forecast_cycle_online,
};
use super::forecast_cycle_page_binding::{BoundForecastCyclePageCodec, ForecastCycleHashPair};
use super::forecast_cycle_pending_final::{
    NativeForecastPendingFinalError, next_native_forecast_final,
};
use super::forecast_cycle_transfer_checkpoint::{
    ForecastCycleTransferCheckpoint, ForecastCycleTransferCheckpointError,
    NativeForecastCycleTransferCheckpoints,
};

/// Native selection and acknowledgement failures below the existing transfer command boundary.
#[derive(Debug)]
pub enum NativeForecastCycleOnlineTransferError {
    RootMismatch,
    LeaseBusy,
    LeaseStorage,
    Selection(ForecastCycleOnlineNextError<NativeForecastTransferStorageError>),
    Storage(NativeForecastTransferStorageError),
    FinalNotPending,
    ClockUnavailable,
    GraphCollision,
    ProofRefused,
}

/// Concrete retained-store failures distinguish absence from refused durable recovery.
#[derive(Debug)]
pub enum NativeForecastTransferStorageError {
    Slot(ForecastCycleDiskSlotError),
    Transfer(ForecastCycleTransferCheckpointError),
    Seal(ForecastCycleInputSealError),
    Manifest(ForecastCycleFinalManifestError),
    Ack(ForecastCycleDiskAckError),
    FinalAck(ForecastCycleFinalAckError),
    Gap(ForecastCycleGapError),
    Reconcile(ForecastCycleReconcileError),
    Finalize(ForecastCycleFinalizationError),
    Pending(NativeForecastPendingFinalError),
    InvalidCycle,
}

/// Use the existing finite native custody handles without adding a second transport.
pub struct NativeForecastCycleOnlineTransfer<'ports, 'codec, Codec, Statfs> {
    slots: &'ports NativeForecastCycleDiskSlots<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
    transfers: &'ports NativeForecastCycleTransferCheckpoints<
        BoundForecastCyclePageCodec<'codec, Codec>,
        Statfs,
    >,
    seals:
        &'ports NativeForecastCycleInputSeals<BoundForecastCyclePageCodec<'codec, Codec>, Statfs>,
    manifests: &'ports mut NativeForecastCycleFinalManifests<
        BoundForecastCyclePageCodec<'codec, Codec>,
        Statfs,
    >,
    final_acks: &'ports mut NativeForecastCycleFinalAcknowledgements<
        BoundForecastCyclePageCodec<'codec, Codec>,
        Statfs,
    >,
    codec: &'ports BoundForecastCyclePageCodec<'codec, Codec>,
}

/// The chosen exact native v1 archive transfer.
pub type NativeForecastOnlineNext = ForecastCycleOnlineNext<ForecastCycleFinalManifest>;

impl<'ports, 'codec, Codec, Statfs> NativeForecastCycleOnlineTransfer<'ports, 'codec, Codec, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: ForecastAllocationStatfsPort,
{
    /// Bind every receipt and value owner to the same explicit archive root.
    pub fn new(
        slots: &'ports NativeForecastCycleDiskSlots<
            BoundForecastCyclePageCodec<'codec, Codec>,
            Statfs,
        >,
        transfers: &'ports NativeForecastCycleTransferCheckpoints<
            BoundForecastCyclePageCodec<'codec, Codec>,
            Statfs,
        >,
        seals: &'ports NativeForecastCycleInputSeals<
            BoundForecastCyclePageCodec<'codec, Codec>,
            Statfs,
        >,
        manifests: &'ports mut NativeForecastCycleFinalManifests<
            BoundForecastCyclePageCodec<'codec, Codec>,
            Statfs,
        >,
        final_acks: &'ports mut NativeForecastCycleFinalAcknowledgements<
            BoundForecastCyclePageCodec<'codec, Codec>,
            Statfs,
        >,
        codec: &'ports BoundForecastCyclePageCodec<'codec, Codec>,
    ) -> Result<Self, NativeForecastCycleOnlineTransferError> {
        let root = slots.root();
        // reject any inconsistent root before taking its shared lease
        if root != transfers.root()
            || root != seals.root()
            || root != manifests.root()
            || root != final_acks.root()
        {
            return Err(NativeForecastCycleOnlineTransferError::RootMismatch);
        }
        Ok(Self {
            slots,
            transfers,
            seals,
            manifests,
            final_acks,
            codec,
        })
    }

    /// Recover and select one native transfer while holding the original transport lease.
    pub fn next(
        &mut self,
        lock_codec: &impl ForecastArchiveLockCodec,
        lock_clock: &mut impl ForecastArchiveLockClock,
    ) -> Result<NativeForecastOnlineNext, NativeForecastCycleOnlineTransferError> {
        let slots = self.slots;
        let transfers = self.transfers;
        with_native_forecast_archive_lease(slots.root(), lock_codec, lock_clock, || {
            next_forecast_cycle_online(slots, transfers, self)
                .map_err(NativeForecastCycleOnlineTransferError::Selection)
        })
        .map_err(lease_error)
    }

    /// Accept only the exact oldest pending final or the same first fsynced graph proof.
    pub fn acknowledge_final(
        &mut self,
        manifest_sha256: ForecastCycleSha256,
        graph_sha256: ForecastCycleSha256,
        lock_codec: &impl ForecastArchiveLockCodec,
        clock: &mut impl RevisionArchiveCanonicalClock,
    ) -> Result<ForecastCycleFinalAcknowledgement, NativeForecastCycleOnlineTransferError> {
        let root = self.slots.root();
        let clock = RefCell::new(clock);
        let mut lease_clock = SharedCanonicalClock(&clock);
        let mut lease_clock = BoundForecastArchiveLockClock::new(&mut lease_clock, self.codec);
        with_native_forecast_archive_lease(root, lock_codec, &mut lease_clock, || {
            self.acknowledge_final_under_lease(manifest_sha256, graph_sha256, || {
                clock.try_borrow_mut().ok()?.canonical_now()
            })
        })
        .map_err(lease_error)
    }

    /// Preserve first-writer acknowledgement identity before looking for another pending cycle.
    fn acknowledge_final_under_lease(
        &mut self,
        manifest_sha256: ForecastCycleSha256,
        graph_sha256: ForecastCycleSha256,
        sample_clock: impl FnOnce() -> Option<String>,
    ) -> Result<ForecastCycleFinalAcknowledgement, NativeForecastCycleOnlineTransferError> {
        use NativeForecastCycleOnlineTransferError as Error;
        use NativeForecastTransferStorageError as Storage;

        let existing = self
            .final_acks
            .read_final_acknowledgement(manifest_sha256)
            .map_err(|error| Error::Storage(Storage::FinalAck(error)))?;
        // return the original first clock on an exact graph retry
        if let Some(existing) = existing {
            return if existing.graph_sha256 == graph_sha256 {
                Ok(existing)
            } else {
                Err(Error::GraphCollision)
            };
        }
        self.recover_acknowledged_finals().map_err(Error::Storage)?;
        let pending = self.next_pending_final_manifest().map_err(Error::Storage)?;
        // do not acknowledge a different or already selected later final
        if pending.is_none_or(|entry| entry.manifest_sha256 != manifest_sha256) {
            return Err(Error::FinalNotPending);
        }
        let acknowledgement = ForecastCycleFinalAcknowledgement {
            acknowledged_at: sample_clock().ok_or(Error::ClockUnavailable)?,
            contract_version: FORECAST_CYCLE_FINAL_ACK_CONTRACT_VERSION,
            graph_sha256,
            manifest_sha256,
        };
        let proof = self
            .final_acks
            .persist_final_acknowledgement(&acknowledgement)
            .map_err(|error| Error::Storage(Storage::FinalAck(error)))?;
        // require the concrete durability owner to return the same final digest
        if !proof.fsynced || proof.manifest_sha256 != manifest_sha256 {
            return Err(Error::ProofRefused);
        }
        Ok(acknowledgement)
    }
}

impl<Codec, Statfs> ForecastCyclePendingFinalProvider
    for NativeForecastCycleOnlineTransfer<'_, '_, Codec, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: ForecastAllocationStatfsPort,
{
    type Manifest = ForecastCycleFinalManifest;
    type Error = NativeForecastTransferStorageError;

    /// Borrow the common custody root checked at construction.
    fn admitted_root(&self) -> &Path {
        self.slots.root()
    }

    /// Close each fully acknowledged generation with a durable seal and no retained slots.
    fn recover_acknowledged_finals(&mut self) -> Result<(), Self::Error> {
        use NativeForecastTransferStorageError as Error;

        let transfers = self
            .transfers
            .list_transfer_acknowledgements()
            .map_err(Error::Transfer)?;
        let current = self
            .slots
            .read_slot(ForecastCyclePageSlotKind::Current)
            .map_err(Error::Slot)?;
        let next = self
            .slots
            .read_slot(ForecastCyclePageSlotKind::Next)
            .map_err(Error::Slot)?;
        let live_slots = [current.as_ref(), next.as_ref()];
        // recover each generation independently in the original transfer census order
        for mut group in group_transfers(transfers) {
            group.sort_by_key(|checkpoint| checkpoint.header.page_index);
            let first = &group[0].header;
            let due_key = first.due_key.clone();
            let generation = first.generation;
            let seal = self
                .seals
                .read_input_seal(&due_key, generation)
                .map_err(Error::Seal)?;
            let Some(seal) = seal else {
                continue;
            };
            // known final or still-retained payload means there is nothing to recover for this group
            if self
                .manifests
                .find_final_manifest(&due_key, generation)
                .map_err(Error::Manifest)?
                .is_some()
                || live_slots.iter().flatten().any(|page| {
                    page.header.due_key == due_key && page.header.generation == generation
                })
            {
                continue;
            }
            self.recover_one_final(group, &seal)?;
        }
        Ok(())
    }

    /// Return only the first final with no durable first graph acknowledgement.
    fn next_pending_final_manifest(
        &mut self,
    ) -> Result<Option<ForecastCyclePendingFinal<Self::Manifest>>, Self::Error> {
        next_native_forecast_final(self.manifests, self.final_acks)
            .map(|value| {
                value.map(|entry| ForecastCyclePendingFinal {
                    manifest: entry.manifest,
                    manifest_sha256: entry.manifest_sha256,
                })
            })
            .map_err(NativeForecastTransferStorageError::Pending)
    }
}

impl<Codec, Statfs> NativeForecastCycleOnlineTransfer<'_, '_, Codec, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: ForecastAllocationStatfsPort,
{
    /// Reconstruct the exact sealed chain without requiring the acknowledged payloads again.
    fn recover_one_final(
        &mut self,
        group: Vec<ForecastCycleTransferCheckpoint>,
        seal: &super::forecast_cycle_input_seal::ForecastCycleInputSeal,
    ) -> Result<(), NativeForecastTransferStorageError> {
        use NativeForecastTransferStorageError as Error;

        let mut acknowledgements = Vec::with_capacity(group.len());
        let mut pages = Vec::with_capacity(group.len());
        // do not close a partial durable page-protocol acknowledgement prefix
        for transfer in group {
            let ack = self
                .slots
                .read_cycle_page_acknowledgement(transfer.page_sha256)
                .map_err(Error::Ack)?;
            let Some(ack) = ack else {
                return Ok(());
            };
            acknowledgements.push(ack);
            pages.push(page_metadata(transfer));
        }
        let hour: usize = seal
            .due_key
            .get(19..21)
            .ok_or(Error::InvalidCycle)?
            .parse()
            .map_err(|_| Error::InvalidCycle)?;
        let initial = create_forecast_cycle_page_state(ForecastCyclePageStateInput {
            daily_page_count: hour / 6,
            due_key: seal.due_key.clone(),
            generation: seal.generation,
            local_date: seal
                .due_key
                .get(8..18)
                .ok_or(Error::InvalidCycle)?
                .to_owned(),
        })
        .ok_or(Error::InvalidCycle)?;
        let gaps = self
            .slots
            .read_cycle_gaps(&seal.due_key, seal.generation)
            .map_err(Error::Gap)?;
        let state = reconcile_forecast_cycle_pages(
            &initial,
            ForecastCycleDurableState::open(pages, acknowledgements),
            self.codec,
        )
        .map_err(Error::Reconcile)?
        .state
        .with_validated_gaps(gaps);
        let prepared =
            prepare_forecast_cycle_final_manifest(&state, &seal.sealed_at, Some(seal), self.codec)
                .map_err(Error::Finalize)?;
        self.manifests
            .persist_final_manifest(&prepared.manifest, prepared.manifest_sha256)
            .map_err(Error::Manifest)?;
        Ok(())
    }
}

/// Preserve first-seen group order without a quadratic scan through prior scope keys.
fn group_transfers(
    transfers: Vec<ForecastCycleTransferCheckpoint>,
) -> Vec<Vec<ForecastCycleTransferCheckpoint>> {
    let mut groups = Vec::<Vec<ForecastCycleTransferCheckpoint>>::new();
    let mut indices = HashMap::new();
    // match the stable insertion order of the source's grouping map
    for transfer in transfers {
        let identity = (transfer.header.due_key.clone(), transfer.header.generation);
        let index = *indices.entry(identity).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[index].push(transfer);
    }
    groups
}

/// Preserve the distinction between custody failure and a guarded transfer refusal.
fn lease_error(
    error: NativeForecastArchiveLeaseError<NativeForecastCycleOnlineTransferError>,
) -> NativeForecastCycleOnlineTransferError {
    use NativeForecastCycleOnlineTransferError as Error;

    match error {
        NativeForecastArchiveLeaseError::Busy => Error::LeaseBusy,
        NativeForecastArchiveLeaseError::Storage(_) => Error::LeaseStorage,
        NativeForecastArchiveLeaseError::Operation(error) => error,
    }
}

/// Reborrow one injected clock for lease sampling without holding it across the operation.
struct SharedCanonicalClock<'a, 'clock, Clock>(&'a RefCell<&'clock mut Clock>);

impl<Clock: RevisionArchiveCanonicalClock> RevisionArchiveCanonicalClock
    for SharedCanonicalClock<'_, '_, Clock>
{
    /// Observe one canonical instant without extending mutable ownership.
    fn canonical_now(&mut self) -> Option<String> {
        self.0.try_borrow_mut().ok()?.canonical_now()
    }
}
