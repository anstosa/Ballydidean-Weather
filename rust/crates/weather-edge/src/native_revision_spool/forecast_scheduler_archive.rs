//! compose the native scheduler archive under one real transport lease.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::forecast_cycle_pages::{
    ForecastCyclePageDurabilityProof, ForecastCyclePagePersistence,
    ForecastCyclePagePersistenceRequest, ForecastCyclePageSlotKind, ForecastCyclePageState,
    ForecastCycleSha256,
};
use crate::forecast_response_capture::{
    ForecastArchiveAppendOutcome, ForecastScheduledEvidenceArchive,
};
use crate::forecast_scheduler_due::ForecastSchedulerCalendarPort;
use crate::revision_archive_canonical::RevisionArchiveCanonicalClock;
use crate::{ForecastEvidenceMaterial, ForecastEvidenceReceiptCodec, PreparedForecastEvidence};

use super::forecast_allocation::ForecastAllocationStatfsPort;
use super::forecast_archive_lock::{
    NativeForecastArchiveLeaseError, with_native_forecast_archive_lease,
};
use super::forecast_archive_lock_binding::{
    BoundForecastArchiveLockClock, BoundForecastArchiveLockCodec,
};
use super::forecast_capture_coordinator::{
    ForecastCaptureCoordinatorInputError, ForecastCaptureCoordinatorPorts,
    ForecastCaptureCoordinatorStatus, ForecastCaptureCustodyError,
    ForecastCycleTransferAcknowledgement, capture_committed_scheduler_evidence,
};
use super::forecast_cycle_disk_recovery::{
    NativeForecastCycleRecoveryError, recover_native_forecast_cycle,
};
use super::forecast_cycle_disk_slots::{
    ForecastCycleDiskSlotError, ForecastCycleDiskSlotPage, NativeForecastCycleDiskSlots,
};
use super::forecast_cycle_final_store::{
    ForecastCycleFinalManifestError, NativeForecastCycleFinalManifests,
};
use super::forecast_cycle_gap::{
    DurableForecastCycleGap, ForecastCycleGapError, ForecastCycleGapReason, ForecastCycleGapRecord,
};
use super::forecast_cycle_input_seal::{
    ForecastCycleInputSealError, NativeForecastCycleInputSeals,
};
use super::forecast_cycle_page_binding::{BoundForecastCyclePageCodec, ForecastCycleHashPair};
use super::forecast_cycle_transfer_checkpoint::{
    ForecastCycleTransferCheckpointError, NativeForecastCycleTransferCheckpoints,
};
use super::forecast_scheduler_archive_payload::{
    ForecastSchedulerArchivePayloadError, read_committed_scheduler_payload,
};

type ReadOnlyTransferStore<'a, Codec> =
    NativeForecastCycleTransferCheckpoints<BoundForecastCyclePageCodec<'a, Codec>, ()>;
type ReadOnlyInputSealStore<'a, Codec> =
    NativeForecastCycleInputSeals<BoundForecastCyclePageCodec<'a, Codec>, ()>;
type ReadOnlyFinalManifestStore<'a, Codec> =
    NativeForecastCycleFinalManifests<BoundForecastCyclePageCodec<'a, Codec>, ()>;

/// closed native failures remain below the scheduler's successful forecast response.
#[derive(Debug)]
pub enum NativeForecastSchedulerArchiveError {
    LeaseBusy,
    LeaseStorage,
    Payload(ForecastSchedulerArchivePayloadError),
    Slot(ForecastCycleDiskSlotError),
    Transfer(ForecastCycleTransferCheckpointError),
    InputSeal(ForecastCycleInputSealError),
    FinalManifest(ForecastCycleFinalManifestError),
    Recovery(NativeForecastCycleRecoveryError),
    RecoveryCurrentChanged,
    Gap(ForecastCycleGapError),
}

/// one real adjustment-evidence archive composed from fixed native storage owners.
pub struct NativeForecastSchedulerArchive<'a, Codec, Calendar, Clock, Statfs> {
    material: &'a ForecastEvidenceMaterial<Codec>,
    calendar: &'a Calendar,
    clock: Rc<RefCell<Clock>>,
    slots: NativeForecastCycleDiskSlots<BoundForecastCyclePageCodec<'a, Codec>, Statfs>,
    transfers: ReadOnlyTransferStore<'a, Codec>,
    input_seals: ReadOnlyInputSealStore<'a, Codec>,
    final_manifests: ReadOnlyFinalManifestStore<'a, Codec>,
}

impl<'a, Codec, Calendar, Clock, Statfs>
    NativeForecastSchedulerArchive<'a, Codec, Calendar, Clock, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
{
    /// initialize only the existing fixed archive directories below one admitted evidence root.
    pub fn initialize(
        root: PathBuf,
        material: &'a ForecastEvidenceMaterial<Codec>,
        calendar: &'a Calendar,
        clock: Clock,
        statfs: Statfs,
    ) -> io::Result<Self> {
        let slots = NativeForecastCycleDiskSlots::initialize(
            root.clone(),
            BoundForecastCyclePageCodec::new(material.codec()),
            statfs,
        )?;
        let transfers = NativeForecastCycleTransferCheckpoints::initialize(
            root.clone(),
            BoundForecastCyclePageCodec::new(material.codec()),
            (),
        )?;
        let input_seals = NativeForecastCycleInputSeals::initialize(
            root.clone(),
            BoundForecastCyclePageCodec::new(material.codec()),
            (),
        )?;
        let final_slots = NativeForecastCycleDiskSlots::initialize(
            root,
            BoundForecastCyclePageCodec::new(material.codec()),
            (),
        )?;
        let final_manifests = NativeForecastCycleFinalManifests::new(final_slots)?;
        Ok(Self {
            material,
            calendar,
            clock: Rc::new(RefCell::new(clock)),
            slots,
            transfers,
            input_seals,
            final_manifests,
        })
    }

    /// borrow the single admitted evidence root shared by every composed owner.
    pub fn root(&self) -> &Path {
        self.slots.root()
    }
}

impl<Codec, Calendar, Clock, Statfs> ForecastCyclePagePersistence
    for NativeForecastSchedulerArchive<'_, Codec, Calendar, Clock, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Statfs: ForecastAllocationStatfsPort,
{
    type Error = ForecastCycleDiskSlotError;

    /// delegate page durability to the one value-bearing fixed-slot owner.
    fn persist_page(
        &mut self,
        request: ForecastCyclePagePersistenceRequest<'_>,
    ) -> Result<ForecastCyclePageDurabilityProof, Self::Error> {
        self.slots.persist_page(request)
    }
}

impl<Codec, Calendar, Clock, Statfs> ForecastCaptureCoordinatorPorts
    for NativeForecastSchedulerArchive<'_, Codec, Calendar, Clock, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Calendar: ForecastSchedulerCalendarPort,
    Clock: RevisionArchiveCanonicalClock,
    Statfs: ForecastAllocationStatfsPort,
{
    type CoordinatorError = NativeForecastSchedulerArchiveError;

    /// hold one Node-compatible transport lease across the complete source-ordered operation.
    fn with_custody<T, OperationError>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, OperationError>,
    ) -> Result<T, ForecastCaptureCustodyError<Self::CoordinatorError, OperationError>> {
        let root = self.slots.root().to_path_buf();
        let material = self.material;
        let calendar = self.calendar;
        let shared_clock = Rc::clone(&self.clock);
        let lock_codec = BoundForecastArchiveLockCodec::new(material.codec(), calendar);
        let mut clock_source = SharedForecastSchedulerArchiveClock(shared_clock);
        let mut lock_clock =
            BoundForecastArchiveLockClock::new(&mut clock_source, material.codec());
        match with_native_forecast_archive_lease(
            &root,
            &lock_codec,
            &mut lock_clock,
            // keep the same concrete storage owners inside the acquired custody lifetime
            || operation(self),
        ) {
            Ok(value) => Ok(value),
            Err(NativeForecastArchiveLeaseError::Operation(error)) => {
                Err(ForecastCaptureCustodyError::Operation(error))
            }
            Err(NativeForecastArchiveLeaseError::Busy) => {
                Err(ForecastCaptureCustodyError::Custody(
                    NativeForecastSchedulerArchiveError::LeaseBusy,
                ))
            }
            Err(NativeForecastArchiveLeaseError::Storage(_)) => {
                Err(ForecastCaptureCustodyError::Custody(
                    NativeForecastSchedulerArchiveError::LeaseStorage,
                ))
            }
        }
    }

    /// rebuild the exact committed object, receipt, assertion, and binding page payload.
    fn read_committed_scheduler_payload(
        &mut self,
        prepared: &PreparedForecastEvidence,
    ) -> Result<Vec<u8>, Self::CoordinatorError> {
        read_committed_scheduler_payload(self.slots.root(), prepared, self.material)
            .map_err(NativeForecastSchedulerArchiveError::Payload)
    }

    /// read only the admitted native current or next slot.
    fn read_cycle_slot(
        &mut self,
        slot: ForecastCyclePageSlotKind,
    ) -> Result<Option<ForecastCycleDiskSlotPage>, Self::CoordinatorError> {
        self.slots
            .read_slot(slot)
            .map_err(NativeForecastSchedulerArchiveError::Slot)
    }

    /// reconstruct one validated frontier from a fresh second transfer census.
    fn recover_open_state(
        &mut self,
        due_key: &str,
        current: Option<&ForecastCycleDiskSlotPage>,
    ) -> Result<ForecastCyclePageState<DurableForecastCycleGap, ()>, Self::CoordinatorError> {
        let codec = BoundForecastCyclePageCodec::new(self.material.codec());
        let state = recover_native_forecast_cycle(due_key, &self.slots, &self.transfers, &codec)
            .map_err(NativeForecastSchedulerArchiveError::Recovery)?
            .into_page_state();

        // bind the second census to the current generation observed before closure checks
        if current.is_some_and(|page| {
            page.header.due_key != state.due_key || page.header.generation != state.generation
        }) {
            return Err(NativeForecastSchedulerArchiveError::RecoveryCurrentChanged);
        }
        Ok(state)
    }

    /// project the source-ordered first checkpoint census without retaining released payloads.
    fn list_transfer_acknowledgements(
        &mut self,
    ) -> Result<Vec<ForecastCycleTransferAcknowledgement>, Self::CoordinatorError> {
        self.transfers
            .list_transfer_acknowledgements()
            .map_err(NativeForecastSchedulerArchiveError::Transfer)
            .map(|checkpoints| {
                checkpoints
                    .into_iter()
                    .map(|checkpoint| ForecastCycleTransferAcknowledgement {
                        header: checkpoint.header,
                        page_sha256: checkpoint.page_sha256,
                        payload_length: checkpoint.payload_length,
                        projections: checkpoint.projections,
                    })
                    .collect()
            })
    }

    /// report only an exact validated producer closure for this due generation.
    fn has_input_seal(
        &mut self,
        due_key: &str,
        generation: u64,
    ) -> Result<bool, Self::CoordinatorError> {
        self.input_seals
            .read_input_seal(due_key, generation)
            .map(|seal| seal.is_some())
            .map_err(NativeForecastSchedulerArchiveError::InputSeal)
    }

    /// report only an exact validated immutable final manifest for this cycle.
    fn has_final_manifest(
        &mut self,
        due_key: &str,
        generation: u64,
    ) -> Result<bool, Self::CoordinatorError> {
        self.final_manifests
            .find_final_manifest(due_key, generation)
            .map(|manifest| manifest.is_some())
            .map_err(NativeForecastSchedulerArchiveError::FinalManifest)
    }

    /// reuse the native complete gap census for idempotent direct refusals.
    fn has_cycle_gap(
        &mut self,
        due_key: &str,
        projection_identity_sha256: ForecastCycleSha256,
    ) -> Result<bool, Self::CoordinatorError> {
        self.slots
            .has_cycle_gap(due_key, projection_identity_sha256)
            .map_err(NativeForecastSchedulerArchiveError::Gap)
    }

    /// sample the same injected canonical clock after an actual gap becomes necessary.
    fn canonical_gap_now(&mut self) -> Option<String> {
        self.clock.try_borrow_mut().ok()?.canonical_now()
    }

    /// retain one first categorical native gap under the current custody decision.
    fn record_cycle_gap(
        &mut self,
        record: ForecastCycleGapRecord,
    ) -> Result<(), Self::CoordinatorError> {
        self.slots
            .record_cycle_gap(record)
            .map(|_| ())
            .map_err(NativeForecastSchedulerArchiveError::Gap)
    }

    /// preserve only the capacity refusal that carries the source resource code.
    fn classify_page_persistence_error(
        &self,
        error: &<Self as ForecastCyclePagePersistence>::Error,
    ) -> ForecastCycleGapReason {
        match error {
            ForecastCycleDiskSlotError::CapacityRefused => ForecastCycleGapReason::CapacityRefused,
            _ => ForecastCycleGapReason::ArchiveRefused,
        }
    }
}

impl<Codec, Calendar, Clock, Statfs> ForecastScheduledEvidenceArchive<PreparedForecastEvidence>
    for NativeForecastSchedulerArchive<'_, Codec, Calendar, Clock, Statfs>
where
    Codec: ForecastEvidenceReceiptCodec + ForecastCycleHashPair,
    Calendar: ForecastSchedulerCalendarPort,
    Clock: RevisionArchiveCanonicalClock,
    Statfs: ForecastAllocationStatfsPort,
{
    type Error = ForecastCaptureCoordinatorInputError;

    /// settle one genuine committed scheduler member after its durable gap or page decision.
    fn append_committed(
        &mut self,
        due_key: &str,
        prepared: &PreparedForecastEvidence,
    ) -> Result<ForecastArchiveAppendOutcome, Self::Error> {
        let material = self.material;
        let codec = BoundForecastCyclePageCodec::new(material.codec());
        let outcome = capture_committed_scheduler_evidence(due_key, prepared, &codec, self)?;
        Ok(
            // expose only the scheduler's existing durable-gap settlement distinction
            if outcome.status == ForecastCaptureCoordinatorStatus::EvidenceGapPersistenceRefused {
                ForecastArchiveAppendOutcome::GapPersistenceRefused
            } else {
                ForecastArchiveAppendOutcome::Settled
            },
        )
    }
}

/// share one injected clock between bounded lease acquisition and later gap observation.
struct SharedForecastSchedulerArchiveClock<Clock>(Rc<RefCell<Clock>>);

impl<Clock: RevisionArchiveCanonicalClock> RevisionArchiveCanonicalClock
    for SharedForecastSchedulerArchiveClock<Clock>
{
    /// reborrow only for the duration of one canonical observation.
    fn canonical_now(&mut self) -> Option<String> {
        self.0.try_borrow_mut().ok()?.canonical_now()
    }
}
