//! Order one committed scheduler capture through custody, retry, recovery, and page append.

use crate::forecast_cycle_pages::{
    ForecastCyclePageAppendInput, ForecastCyclePageCodec, ForecastCyclePageError,
    ForecastCyclePagePersistence, ForecastCyclePageRefusal, ForecastCyclePageState,
    ForecastCycleProjection, ForecastCycleProjectionChannel, ForecastCycleSha256,
    append_forecast_cycle_page, valid_due_key,
};
use crate::{ForecastCaptureChannel, PreparedForecastEvidence};

use super::forecast_cycle_disk_slots::ForecastCycleDiskSlotPage;
use super::forecast_cycle_gap::{
    DurableForecastCycleGap, ForecastCycleGapReason, ForecastCycleGapRecord,
};

/// one workstation-checkpointed page metadata record without its released payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastCycleTransferAcknowledgement {
    pub header: crate::forecast_cycle_pages::ForecastCyclePageHeader,
    pub page_sha256: ForecastCycleSha256,
    pub payload_length: usize,
    pub projections: Vec<ForecastCycleProjection>,
}

/// exact public fail-open status from one local archive capture attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCaptureCoordinatorStatus {
    PagePendingAcknowledgement,
    PageCheckpointed,
    EvidenceGap,
    EvidenceGapPersistenceRefused,
}

impl ForecastCaptureCoordinatorStatus {
    /// return the exact source status spelling.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::PagePendingAcknowledgement => "page_pending_acknowledgement",
            Self::PageCheckpointed => "page_checkpointed",
            Self::EvidenceGap => "evidence_gap",
            Self::EvidenceGapPersistenceRefused => "evidence_gap_persistence_refused",
        }
    }
}

/// nonblocking serving result plus one reason only when gap persistence was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastCaptureCoordinatorOutcome {
    pub serving_blocked: bool,
    pub status: ForecastCaptureCoordinatorStatus,
    pub reason: Option<ForecastCycleGapReason>,
}

/// input failures retained outside the source's fail-open archive boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCaptureCoordinatorInputError {
    InvalidDueKey,
    InvalidIssuanceChannel,
}

/// distinguish custody acquisition or release failure from the guarded operation.
#[derive(Debug)]
pub enum ForecastCaptureCustodyError<CustodyError, OperationError> {
    Custody(CustodyError),
    Operation(OperationError),
}

/// internal guarded-operation faults collapsed to durable archive gaps by the coordinator.
#[derive(Debug)]
pub enum ForecastCaptureCoordinatorOperationError<PortError> {
    Port(PortError),
    InvalidCycleState,
}

/// local custody, metadata, gap, and page-persistence boundary for one coordinator.
pub trait ForecastCaptureCoordinatorPorts: ForecastCyclePagePersistence {
    type CoordinatorError;

    /// run one operation under the exact archive lease and release it before returning.
    fn with_custody<T, OperationError>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, OperationError>,
    ) -> Result<T, ForecastCaptureCustodyError<Self::CoordinatorError, OperationError>>;

    /// read one committed scheduler payload from its exact retained object graph.
    fn read_committed_scheduler_payload(
        &mut self,
        prepared: &PreparedForecastEvidence,
    ) -> Result<Vec<u8>, Self::CoordinatorError>;

    /// read one exact current or next payload slot.
    fn read_cycle_slot(
        &mut self,
        slot: crate::forecast_cycle_pages::ForecastCyclePageSlotKind,
    ) -> Result<Option<ForecastCycleDiskSlotPage>, Self::CoordinatorError>;

    /// recover one validated open state from a fresh second transfer census and both slots.
    fn recover_open_state(
        &mut self,
        due_key: &str,
        current: Option<&ForecastCycleDiskSlotPage>,
    ) -> Result<ForecastCyclePageState<DurableForecastCycleGap, ()>, Self::CoordinatorError>;

    /// list every validated workstation transfer acknowledgement.
    fn list_transfer_acknowledgements(
        &mut self,
    ) -> Result<Vec<ForecastCycleTransferAcknowledgement>, Self::CoordinatorError>;

    /// report whether the exact producer input population is already sealed.
    fn has_input_seal(
        &mut self,
        due_key: &str,
        generation: u64,
    ) -> Result<bool, Self::CoordinatorError>;

    /// report whether this cycle generation already has an immutable final manifest.
    fn has_final_manifest(
        &mut self,
        due_key: &str,
        generation: u64,
    ) -> Result<bool, Self::CoordinatorError>;

    /// report whether any first categorical gap already exists for this projection.
    fn has_cycle_gap(
        &mut self,
        due_key: &str,
        projection_identity_sha256: ForecastCycleSha256,
    ) -> Result<bool, Self::CoordinatorError>;

    /// sample one canonical occurrence instant only when a new gap is required.
    fn canonical_gap_now(&mut self) -> Option<String>;

    /// retain one first categorical gap record.
    fn record_cycle_gap(
        &mut self,
        record: ForecastCycleGapRecord,
    ) -> Result<(), Self::CoordinatorError>;

    /// map one concrete page-persistence error to the source's closed gap category.
    fn classify_page_persistence_error(
        &self,
        error: &<Self as ForecastCyclePagePersistence>::Error,
    ) -> ForecastCycleGapReason;
}

/// capture one already committed scheduler projection without blocking forecast serving.
pub fn capture_committed_scheduler_evidence<Ports, Codec>(
    due_key: &str,
    prepared: &PreparedForecastEvidence,
    codec: &Codec,
    ports: &mut Ports,
) -> Result<ForecastCaptureCoordinatorOutcome, ForecastCaptureCoordinatorInputError>
where
    Ports: ForecastCaptureCoordinatorPorts,
    Codec: ForecastCyclePageCodec,
{
    // retain source input validation outside the fail-open archive operation
    if !valid_due_key(due_key) {
        return Err(ForecastCaptureCoordinatorInputError::InvalidDueKey);
    }
    if prepared.issuance_channel() != Some(ForecastCaptureChannel::SchedulerRequest) {
        return Err(ForecastCaptureCoordinatorInputError::InvalidIssuanceChannel);
    }
    let identity_sha256 = ForecastCycleSha256::admitted(prepared.edge_receipt_identity_sha256());
    let guarded = ports.with_custody(
        // keep every payload and cycle metadata decision under one release-checked custody scope
        |ports| capture_under_custody(due_key, prepared, identity_sha256, codec, ports),
    );
    // convert acquisition, operation, or release failure to one honest permanent gap
    Ok(match guarded {
        Ok(outcome) => outcome,
        Err(_) => record_direct_gap(
            ports,
            due_key,
            identity_sha256,
            ForecastCycleGapReason::ArchiveRefused,
            codec,
        ),
    })
}

/// execute the exact retry and closure checks while archive custody remains held.
fn capture_under_custody<Ports, Codec>(
    due_key: &str,
    prepared: &PreparedForecastEvidence,
    identity_sha256: ForecastCycleSha256,
    codec: &Codec,
    ports: &mut Ports,
) -> Result<
    ForecastCaptureCoordinatorOutcome,
    ForecastCaptureCoordinatorOperationError<Ports::CoordinatorError>,
>
where
    Ports: ForecastCaptureCoordinatorPorts,
    Codec: ForecastCyclePageCodec,
{
    use ForecastCaptureCoordinatorOperationError as Error;

    let payload = ports
        .read_committed_scheduler_payload(prepared)
        .map_err(Error::Port)?;
    let current = ports
        .read_cycle_slot(crate::forecast_cycle_pages::ForecastCyclePageSlotKind::Current)
        .map_err(Error::Port)?;

    // never reuse one pending slot for a later due cycle
    if current
        .as_ref()
        .is_some_and(|page| page.header.due_key != due_key)
    {
        return Ok(record_direct_gap(
            ports,
            due_key,
            identity_sha256,
            ForecastCycleGapReason::SlotUnavailable,
            codec,
        ));
    }

    // make one exact current-slot crash retry a no-op instead of a duplicate projection
    if let Some(page) = current.as_ref()
        && page
            .projections
            .iter()
            .any(|projection| projection.identity_sha256 == identity_sha256)
    {
        if page.header.payload_sha256 != ForecastCycleSha256::admitted(codec.sha256(&payload)) {
            return Ok(record_direct_gap(
                ports,
                due_key,
                identity_sha256,
                ForecastCycleGapReason::ArchiveRefused,
                codec,
            ));
        }
        return Ok(outcome(
            ForecastCaptureCoordinatorStatus::PagePendingAcknowledgement,
        ));
    }
    let prior_transfers = ports
        .list_transfer_acknowledgements()
        .map_err(Error::Port)?
        .into_iter()
        .filter(
            // retain only this due key's immutable original generation candidates
            |transfer| transfer.header.due_key == due_key,
        )
        .collect::<Vec<_>>();
    let generation = current
        .as_ref()
        .map(|page| page.header.generation)
        .or_else(|| {
            prior_transfers
                .first()
                .map(|transfer| transfer.header.generation)
        })
        .or_else(|| generation_from_due_key(due_key, codec))
        .ok_or(Error::InvalidCycleState)?;

    // preserve an exact workstation-checkpointed retry without relabeling first issuance
    if let Some(prior) = prior_transfers.iter().find(|transfer| {
        transfer
            .projections
            .iter()
            .any(|projection| projection.identity_sha256 == identity_sha256)
    }) {
        if prior.header.payload_sha256 != ForecastCycleSha256::admitted(codec.sha256(&payload)) {
            return Ok(record_direct_gap(
                ports,
                due_key,
                identity_sha256,
                ForecastCycleGapReason::ArchiveRefused,
                codec,
            ));
        }
        return Ok(outcome(ForecastCaptureCoordinatorStatus::PageCheckpointed));
    }
    let sealed = ports
        .has_input_seal(due_key, generation)
        .map_err(Error::Port)?;

    // permanently refuse new inputs after explicit producer closure
    if sealed {
        return Ok(record_direct_gap(
            ports,
            due_key,
            identity_sha256,
            ForecastCycleGapReason::ArchiveRefused,
            codec,
        ));
    }
    let finalized = ports
        .has_final_manifest(due_key, generation)
        .map_err(Error::Port)?;

    // never backfill or reopen one already finalized causal cycle
    if finalized {
        return Ok(record_direct_gap(
            ports,
            due_key,
            identity_sha256,
            ForecastCycleGapReason::ArchiveRefused,
            codec,
        ));
    }
    // recensus after closure checks so the native owner sees every concurrent checkpoint
    let state = ports
        .recover_open_state(due_key, current.as_ref())
        .map_err(Error::Port)?;
    let projection = [ForecastCycleProjection {
        channel: ForecastCycleProjectionChannel::SchedulerRequest,
        identity_sha256,
    }];
    match append_forecast_cycle_page(
        &state,
        ForecastCyclePageAppendInput {
            payload: &payload,
            projections: &projection,
        },
        codec,
        ports,
    ) {
        Ok(_) => Ok(outcome(
            ForecastCaptureCoordinatorStatus::PagePendingAcknowledgement,
        )),
        Err(error) => {
            let reason = classify_append_error(ports, &error);
            Ok(record_append_gap(
                ports,
                due_key,
                state.generation,
                identity_sha256,
                reason,
            ))
        }
    }
}

/// classify one append failure using only source gap categories.
fn classify_append_error<Ports: ForecastCaptureCoordinatorPorts>(
    ports: &Ports,
    error: &ForecastCyclePageError<<Ports as ForecastCyclePagePersistence>::Error>,
) -> ForecastCycleGapReason {
    match error {
        ForecastCyclePageError::Refused(ForecastCyclePageRefusal::SlotUnavailable) => {
            ForecastCycleGapReason::SlotUnavailable
        }
        ForecastCyclePageError::Refused(ForecastCyclePageRefusal::PageLimit) => {
            ForecastCycleGapReason::PageLimit
        }
        ForecastCyclePageError::Refused(ForecastCyclePageRefusal::ProjectionLimit) => {
            ForecastCycleGapReason::ProjectionLimit
        }
        ForecastCyclePageError::Persistence(error) => ports.classify_page_persistence_error(error),
        ForecastCyclePageError::InvalidState
        | ForecastCyclePageError::InvalidInput
        | ForecastCyclePageError::Codec
        | ForecastCyclePageError::Refused(_) => ForecastCycleGapReason::ArchiveRefused,
    }
}

/// retain a direct transport refusal using the source fallback generation.
fn record_direct_gap<Ports, Codec>(
    ports: &mut Ports,
    due_key: &str,
    projection_identity_sha256: ForecastCycleSha256,
    reason: ForecastCycleGapReason,
    codec: &Codec,
) -> ForecastCaptureCoordinatorOutcome
where
    Ports: ForecastCaptureCoordinatorPorts,
    Codec: ForecastCyclePageCodec,
{
    // reuse one prior categorical gap before sampling or changing its occurrence clock
    match ports.has_cycle_gap(due_key, projection_identity_sha256) {
        Ok(true) => return outcome(ForecastCaptureCoordinatorStatus::EvidenceGap),
        Ok(false) => {}
        Err(_) => return gap_persistence_refused(reason),
    }
    let Some(generation) = generation_from_due_key(due_key, codec) else {
        return gap_persistence_refused(reason);
    };
    record_new_gap(
        ports,
        due_key,
        generation,
        projection_identity_sha256,
        reason,
    )
}

/// retain an append refusal under the already selected original cycle generation.
fn record_append_gap<Ports: ForecastCaptureCoordinatorPorts>(
    ports: &mut Ports,
    due_key: &str,
    generation: u64,
    projection_identity_sha256: ForecastCycleSha256,
    reason: ForecastCycleGapReason,
) -> ForecastCaptureCoordinatorOutcome {
    record_new_gap(
        ports,
        due_key,
        generation,
        projection_identity_sha256,
        reason,
    )
}

/// sample and persist one first categorical gap without surfacing its failure to serving.
fn record_new_gap<Ports: ForecastCaptureCoordinatorPorts>(
    ports: &mut Ports,
    due_key: &str,
    generation: u64,
    projection_identity_sha256: ForecastCycleSha256,
    reason: ForecastCycleGapReason,
) -> ForecastCaptureCoordinatorOutcome {
    let Some(occurred_at) = ports.canonical_gap_now() else {
        return gap_persistence_refused(reason);
    };
    let record = ForecastCycleGapRecord {
        due_key: due_key.to_owned(),
        generation,
        projection_identity_sha256,
        reason,
        occurred_at,
    };
    match ports.record_cycle_gap(record) {
        Ok(()) => outcome(ForecastCaptureCoordinatorStatus::EvidenceGap),
        Err(_) => gap_persistence_refused(reason),
    }
}

/// create one ordinary nonblocking status without a refusal reason field.
const fn outcome(status: ForecastCaptureCoordinatorStatus) -> ForecastCaptureCoordinatorOutcome {
    ForecastCaptureCoordinatorOutcome {
        serving_blocked: false,
        status,
        reason: None,
    }
}

/// retain the original refusal category only when its durable gap could not be written.
const fn gap_persistence_refused(
    reason: ForecastCycleGapReason,
) -> ForecastCaptureCoordinatorOutcome {
    ForecastCaptureCoordinatorOutcome {
        serving_blocked: false,
        status: ForecastCaptureCoordinatorStatus::EvidenceGapPersistenceRefused,
        reason: Some(reason),
    }
}

/// derive the original millisecond generation through the approved UTC owner.
fn generation_from_due_key(due_key: &str, codec: &impl ForecastCyclePageCodec) -> Option<u64> {
    let instant = due_key.get(8..)?;
    u64::try_from(codec.canonical_utc_milliseconds(instant)?).ok()
}
