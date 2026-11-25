//! Attach the three production archive verbs to the real local Rust custody owners.

#[path = "forecast_archive_seal_impl.rs"]
mod seal;

use std::path::PathBuf;

use crate::ForecastEvidenceReceiptCodec;
use crate::forecast_cycle_pages::ForecastCycleSha256;
use crate::forecast_scheduler_due::ForecastSchedulerCalendarPort;
use crate::revision_archive_canonical::RevisionArchiveCanonicalClock;

use super::forecast_allocation::ForecastAllocationStatfsPort;
use super::forecast_archive_lock_binding::{
    BoundForecastArchiveLockClock, BoundForecastArchiveLockCodec,
};
use super::forecast_archive_operator::ForecastArchiveOperatorApplication;
use super::forecast_cycle_disk_ack::NativeForecastCycleDiskAcknowledgements;
use super::forecast_cycle_disk_slots::NativeForecastCycleDiskSlots;
use super::forecast_cycle_final_ack::NativeForecastCycleFinalAcknowledgements;
use super::forecast_cycle_final_store::NativeForecastCycleFinalManifests;
use super::forecast_cycle_input_seal::NativeForecastCycleInputSeals;
use super::forecast_cycle_online_next::ForecastCyclePendingFinalProvider;
use super::forecast_cycle_online_page_ack::{
    ForecastCycleOnlinePageAckError, ForecastCycleOnlinePageAckPorts,
    ForecastCycleOnlinePageAckRequest, acknowledge_forecast_cycle_online_page,
};
use super::forecast_cycle_online_transfer::{
    NativeForecastCycleOnlineTransfer, NativeForecastCycleOnlineTransferError,
};
use super::forecast_cycle_online_wire::{
    ForecastCycleOnlineWireError, encode_forecast_cycle_online_next,
};
use super::forecast_cycle_page_binding::{BoundForecastCyclePageCodec, ForecastCycleHashPair};
use super::forecast_cycle_transfer_checkpoint::NativeForecastCycleTransferCheckpoints;

type NativeSlots<'a, Codec, Statfs> =
    NativeForecastCycleDiskSlots<BoundForecastCyclePageCodec<'a, Codec>, Statfs>;
type NativePageAcks<'a, Codec, Statfs> = NativeForecastCycleDiskAcknowledgements<
    NativeSlots<'a, Codec, Statfs>,
    BoundForecastCyclePageCodec<'a, Codec>,
    Statfs,
>;

/// Ordinary command failures retain custody, typed acknowledgement, or canonical output scope.
#[derive(Debug)]
pub enum NativeForecastArchiveOperatorError {
    Transfer(NativeForecastCycleOnlineTransferError),
    PageAck(ForecastCycleOnlinePageAckError<NativeForecastCycleOnlineTransferError>),
    Wire(ForecastCycleOnlineWireError),
}

/// Existing online archive operator actions wired to a single admitted local evidence root.
pub struct NativeForecastArchiveOperator<'a, Codec, Calendar, Clock, Statfs> {
    bytes: &'a Codec,
    calendar: &'a Calendar,
    clock: Clock,
    page_acks: NativePageAcks<'a, Codec, Statfs>,
    transfers:
        NativeForecastCycleTransferCheckpoints<BoundForecastCyclePageCodec<'a, Codec>, Statfs>,
    seals: NativeForecastCycleInputSeals<BoundForecastCyclePageCodec<'a, Codec>, Statfs>,
    manifests: NativeForecastCycleFinalManifests<BoundForecastCyclePageCodec<'a, Codec>, Statfs>,
    final_acks:
        NativeForecastCycleFinalAcknowledgements<BoundForecastCyclePageCodec<'a, Codec>, Statfs>,
}

impl<'a, Codec, Calendar, Clock, Statfs>
    NativeForecastArchiveOperator<'a, Codec, Calendar, Clock, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: Clone,
{
    /// Bind the same approved statfs source to the fixed native archive directories.
    pub fn initialize(
        root: PathBuf,
        bytes: &'a Codec,
        calendar: &'a Calendar,
        clock: Clock,
        statfs: Statfs,
    ) -> std::io::Result<Self> {
        let codec = || BoundForecastCyclePageCodec::new(bytes);
        let slots =
            NativeForecastCycleDiskSlots::initialize(root.clone(), codec(), statfs.clone())?;
        let page_acks =
            NativeForecastCycleDiskAcknowledgements::new(slots, codec(), statfs.clone());
        let transfers = NativeForecastCycleTransferCheckpoints::initialize(
            root.clone(),
            codec(),
            statfs.clone(),
        )?;
        let seals =
            NativeForecastCycleInputSeals::initialize(root.clone(), codec(), statfs.clone())?;
        let final_slots =
            NativeForecastCycleDiskSlots::initialize(root.clone(), codec(), statfs.clone())?;
        let manifests = NativeForecastCycleFinalManifests::new(final_slots)?;
        let final_acks =
            NativeForecastCycleFinalAcknowledgements::initialize(root, codec(), statfs)?;
        Ok(Self {
            bytes,
            calendar,
            clock,
            page_acks,
            transfers,
            seals,
            manifests,
            final_acks,
        })
    }
}

impl<Codec, Calendar, Clock, Statfs> ForecastArchiveOperatorApplication
    for NativeForecastArchiveOperator<'_, Codec, Calendar, Clock, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Calendar: ForecastSchedulerCalendarPort,
    Clock: RevisionArchiveCanonicalClock,
    Statfs: ForecastAllocationStatfsPort,
{
    type Error = NativeForecastArchiveOperatorError;

    /// Select one exact native envelope and keep its original canonical framing.
    fn next_archive_bytes(&mut self) -> Result<Vec<u8>, Self::Error> {
        use NativeForecastArchiveOperatorError as Error;
        let codec = BoundForecastCyclePageCodec::new(self.bytes);
        let lock = BoundForecastArchiveLockCodec::new(self.bytes, self.calendar);
        let mut clock = BoundForecastArchiveLockClock::new(&mut self.clock, self.bytes);
        let mut transfer = NativeForecastCycleOnlineTransfer::new(
            self.page_acks.slots(),
            &self.transfers,
            &self.seals,
            &mut self.manifests,
            &mut self.final_acks,
            &codec,
        )
        .map_err(Error::Transfer)?;
        let value = transfer.next(&lock, &mut clock).map_err(Error::Transfer)?;
        encode_forecast_cycle_online_next(&value, &codec).map_err(Error::Wire)
    }

    /// Release only the recorded physical page after its first transfer checkpoint.
    fn acknowledge_archive_page(
        &mut self,
        page_sha256: ForecastCycleSha256,
        checkpoint_sha256: ForecastCycleSha256,
    ) -> Result<(), Self::Error> {
        let codec = BoundForecastCyclePageCodec::new(self.bytes);
        let lock = BoundForecastArchiveLockCodec::new(self.bytes, self.calendar);
        let seals = &self.seals;
        let manifests = &mut self.manifests;
        let final_acks = &mut self.final_acks;
        acknowledge_forecast_cycle_online_page(
            ForecastCycleOnlinePageAckRequest {
                page_sha256,
                checkpoint_sha256,
            },
            ForecastCycleOnlinePageAckPorts::new(
                &mut self.page_acks,
                &mut self.transfers,
                &codec,
                &mut self.clock,
                &lock,
            ),
            // recover final closure under the page acknowledgement's already held lease
            |slots, transfers| {
                let mut provider = NativeForecastCycleOnlineTransfer::new(
                    slots, transfers, seals, manifests, final_acks, &codec,
                )?;
                provider
                    .recover_acknowledged_finals()
                    .map_err(NativeForecastCycleOnlineTransferError::Storage)
            },
        )
        .map(|_| ())
        .map_err(NativeForecastArchiveOperatorError::PageAck)
    }

    /// Bind only the first admitted cold graph for the current pending final.
    fn acknowledge_archive_final(
        &mut self,
        manifest_sha256: ForecastCycleSha256,
        graph_sha256: ForecastCycleSha256,
    ) -> Result<(), Self::Error> {
        use NativeForecastArchiveOperatorError as Error;
        let codec = BoundForecastCyclePageCodec::new(self.bytes);
        let lock = BoundForecastArchiveLockCodec::new(self.bytes, self.calendar);
        let mut transfer = NativeForecastCycleOnlineTransfer::new(
            self.page_acks.slots(),
            &self.transfers,
            &self.seals,
            &mut self.manifests,
            &mut self.final_acks,
            &codec,
        )
        .map_err(Error::Transfer)?;
        transfer
            .acknowledge_final(manifest_sha256, graph_sha256, &lock, &mut self.clock)
            .map(|_| ())
            .map_err(Error::Transfer)
    }
}
