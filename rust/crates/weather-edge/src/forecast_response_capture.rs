//! Post-response capture ordering shared by the forecast listener and scheduler.

use weather_contracts::JsonValue;

use crate::{
    PublicServingApplicationBody, PublicServingApplicationResponse, RevisionArchiveCanonicalClock,
};

/// one forecast query shape understood by the retained immutable capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastEvidenceWindow {
    OneDay,
    FiveDays,
    TenDays,
    Overnight,
}

impl ForecastEvidenceWindow {
    /// return the existing capture contract's exact requested window.
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::OneDay => "days=1",
            Self::FiveDays => "days=5",
            Self::TenDays => "days=10",
            Self::Overnight => "overnight",
        }
    }
}

/// normalize already decoded URL search fields in their original order.
pub fn normalize_forecast_evidence_window(
    query: &[(&str, &str)],
) -> Option<ForecastEvidenceWindow> {
    match query {
        [] | [("days", "1")] => Some(ForecastEvidenceWindow::OneDay),
        [("days", "5")] => Some(ForecastEvidenceWindow::FiveDays),
        [("days", "10")] => Some(ForecastEvidenceWindow::TenDays),
        [("window", "overnight")] => Some(ForecastEvidenceWindow::Overnight),
        _ => None,
    }
}

/// evidence channel selected by the trusted scheduler owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCaptureChannel {
    PublicGet,
    SchedulerRequest,
}

/// the active capture generation and its runtime-selected issuance channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastCaptureVersion {
    Legacy,
    ChannelBound(ForecastCaptureChannel),
}

/// an admitted in-process scheduler request, not derived from public input here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedForecastScheduleCapture {
    due_key: String,
}

impl AdmittedForecastScheduleCapture {
    /// bind the existing scheduler's admitted due key after request association.
    pub fn admitted(due_key: impl Into<String>) -> Self {
        Self {
            due_key: due_key.into(),
        }
    }

    /// borrow the exact schedule slot assigned to the successful response.
    pub fn due_key(&self) -> &str {
        &self.due_key
    }
}

/// use an already projected value when available rather than decoding its wire again.
#[derive(Debug, Clone, Copy)]
pub enum ForecastCaptureSource<'a> {
    Projected(&'a JsonValue),
    Encoded(&'a [u8]),
}

/// terminal evidence results recognized by the current scheduler and evidence counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastEvidenceOutcome {
    Created,
    Duplicate,
    StoreUnavailable,
    IdentityCollision,
    ObjectTooLarge,
    CapacityExhausted,
    PreparationRefused,
    ResponseAborted,
    WriteFailed,
}

impl ForecastEvidenceOutcome {
    /// return the exact result used by the retained scheduler contract.
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Duplicate => "duplicate",
            Self::StoreUnavailable => "store_unavailable",
            Self::IdentityCollision => "identity_collision",
            Self::ObjectTooLarge => "object_too_large",
            Self::CapacityExhausted => "capacity_exhausted",
            Self::PreparationRefused => "preparation_refused",
            Self::ResponseAborted => "response_aborted",
            Self::WriteFailed => "write_failed",
        }
    }
}

/// evidence returned to an actual scheduled slot after optional archive settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastScheduleSettlement {
    Evidence(ForecastEvidenceOutcome),
    ArchiveUnavailable,
    ArchiveGapUnrecorded,
}

impl ForecastScheduleSettlement {
    /// return the exact retained scheduler outcome.
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Evidence(evidence) => evidence.wire_value(),
            Self::ArchiveUnavailable => "archive_unavailable",
            Self::ArchiveGapUnrecorded => "archive_gap_unrecorded",
        }
    }
}

/// exact synchronous preparation and later bounded serialized evidence writer.
pub trait ForecastResponseEvidenceStore {
    type Prepared;
    type Error;

    /// prepare immutable evidence before response emission, recording ordinary refusal internally.
    fn prepare(
        &mut self,
        source: ForecastCaptureSource<'_>,
        window: Option<ForecastEvidenceWindow>,
        version: ForecastCaptureVersion,
    ) -> Option<Self::Prepared>;

    /// commit the prepared material using the clock sampled at response finish.
    fn commit(
        &mut self,
        prepared: &Self::Prepared,
        first_edge_committed_at: &str,
    ) -> Result<ForecastEvidenceOutcome, Self::Error>;

    /// record a close that arrived before successful response emission.
    fn note_close_without_finish(&mut self);

    /// record the bounded asynchronous failure and gap counters without exposing its cause.
    fn note_write_failure(&mut self);
}

/// a bounded result from appending committed scheduled evidence to the typed archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastArchiveAppendOutcome {
    Settled,
    GapPersistenceRefused,
}

/// optional archive writer for a genuinely committed scheduled response.
pub trait ForecastScheduledEvidenceArchive<Prepared> {
    type Error;

    /// append only an exact already committed evidence member for this due key.
    fn append_committed(
        &mut self,
        due_key: &str,
        prepared: &Prepared,
    ) -> Result<ForecastArchiveAppendOutcome, Self::Error>;
}

/// retain whether archive transfer is disabled, requested but unavailable, or ready.
pub enum ForecastScheduleArchive<'a, Archive> {
    Disabled,
    Unavailable,
    Ready(&'a mut Archive),
}

/// one response lifecycle observer, consumed by the first real terminal event.
pub struct PendingForecastResponseCapture<Prepared> {
    prepared: Option<Prepared>,
    scheduled: Option<AdmittedForecastScheduleCapture>,
}

/// one native listener event after a registered response observer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastResponseTerminal {
    Finished,
    Closed,
}

/// prepare the real Rust public response for evidence capture before sending its bytes.
pub fn prepare_forecast_response_capture<Store: ForecastResponseEvidenceStore>(
    response: &PublicServingApplicationResponse,
    projected: Option<&JsonValue>,
    query: &[(&str, &str)],
    channel_bound_capture_active: bool,
    scheduled: Option<AdmittedForecastScheduleCapture>,
    store: &mut Store,
) -> Option<PendingForecastResponseCapture<Store::Prepared>> {
    prepare_forecast_response_capture_for_window(
        response,
        projected,
        normalize_forecast_evidence_window(query),
        channel_bound_capture_active,
        scheduled,
        store,
    )
}

/// Prepare the matching response using the window decoded from its own admitted search.
pub(crate) fn prepare_forecast_response_capture_for_window<Store: ForecastResponseEvidenceStore>(
    response: &PublicServingApplicationResponse,
    projected: Option<&JsonValue>,
    window: Option<ForecastEvidenceWindow>,
    channel_bound_capture_active: bool,
    scheduled: Option<AdmittedForecastScheduleCapture>,
    store: &mut Store,
) -> Option<PendingForecastResponseCapture<Store::Prepared>> {
    // only the public API owner flags a successful forecast get for capture
    if !response.track_successful_forecast_response {
        return None;
    }
    let PublicServingApplicationBody::Bytes(Some(bytes)) = &response.body else {
        return None;
    };
    let version = if channel_bound_capture_active {
        ForecastCaptureVersion::ChannelBound(if scheduled.is_some() {
            ForecastCaptureChannel::SchedulerRequest
        } else {
            ForecastCaptureChannel::PublicGet
        })
    } else {
        ForecastCaptureVersion::Legacy
    };
    let source = projected.map_or(
        ForecastCaptureSource::Encoded(bytes),
        ForecastCaptureSource::Projected,
    );
    let prepared = store.prepare(source, window, version);
    Some(PendingForecastResponseCapture {
        prepared,
        scheduled,
    })
}

/// an owned action to enqueue on the existing isolated post-response writer lane.
pub struct ForecastPostResponseWork<Prepared> {
    scheduled: Option<AdmittedForecastScheduleCapture>,
    operation: ForecastPostResponseOperation<Prepared>,
}

enum ForecastPostResponseOperation<Prepared> {
    Commit {
        prepared: Prepared,
        first_edge_committed_at: String,
    },
    Settle(ForecastEvidenceOutcome),
}

/// convert a real finish/close event to first-wins work without performing file writes.
pub fn observe_forecast_response_terminal<Store: ForecastResponseEvidenceStore>(
    pending: &mut Option<PendingForecastResponseCapture<Store::Prepared>>,
    terminal: ForecastResponseTerminal,
    clock: &mut impl RevisionArchiveCanonicalClock,
    store: &mut Store,
) -> Option<ForecastPostResponseWork<Store::Prepared>> {
    let PendingForecastResponseCapture {
        prepared,
        scheduled,
    } = pending.take()?;
    let operation = match (terminal, prepared) {
        (ForecastResponseTerminal::Closed, _) => {
            store.note_close_without_finish();
            ForecastPostResponseOperation::Settle(ForecastEvidenceOutcome::ResponseAborted)
        }
        (ForecastResponseTerminal::Finished, None) => {
            ForecastPostResponseOperation::Settle(ForecastEvidenceOutcome::PreparationRefused)
        }
        (ForecastResponseTerminal::Finished, Some(prepared)) => match clock.canonical_now() {
            Some(first_edge_committed_at) => ForecastPostResponseOperation::Commit {
                prepared,
                first_edge_committed_at,
            },
            None => {
                store.note_write_failure();
                ForecastPostResponseOperation::Settle(ForecastEvidenceOutcome::WriteFailed)
            }
        },
    };
    Some(ForecastPostResponseWork {
        scheduled,
        operation,
    })
}

/// evidence and optional scheduled slot settlement after all applicable write work.
#[derive(Debug, PartialEq, Eq)]
pub struct CompletedForecastResponseCapture {
    pub evidence: ForecastEvidenceOutcome,
    pub scheduled: Option<CompletedForecastScheduleCapture>,
}

/// result associated with the original scheduler slot.
#[derive(Debug, PartialEq, Eq)]
pub struct CompletedForecastScheduleCapture {
    pub slot: AdmittedForecastScheduleCapture,
    pub settlement: ForecastScheduleSettlement,
}

/// run one owned work item on the post-response persistence lane, never before response finish.
pub fn persist_forecast_response_capture<Store, Archive>(
    work: ForecastPostResponseWork<Store::Prepared>,
    store: &mut Store,
    archive: ForecastScheduleArchive<'_, Archive>,
) -> CompletedForecastResponseCapture
where
    Store: ForecastResponseEvidenceStore,
    Archive: ForecastScheduledEvidenceArchive<Store::Prepared>,
{
    let (evidence, committed) = match work.operation {
        ForecastPostResponseOperation::Settle(outcome) => (outcome, None),
        ForecastPostResponseOperation::Commit {
            prepared,
            first_edge_committed_at,
        } => {
            let outcome = match store.commit(&prepared, &first_edge_committed_at) {
                Ok(outcome) => outcome,
                Err(_) => {
                    store.note_write_failure();
                    ForecastEvidenceOutcome::WriteFailed
                }
            };
            (outcome, Some(prepared))
        }
    };

    let scheduled = work.scheduled.map(
        // settle only a recognized scheduler request, retaining the original evidence on archive failure
        |slot| {
            let settlement = match (archive, committed.as_ref(), evidence) {
                (ForecastScheduleArchive::Unavailable, _, _) => {
                    ForecastScheduleSettlement::ArchiveUnavailable
                }
                (
                    ForecastScheduleArchive::Ready(archive),
                    Some(prepared),
                    ForecastEvidenceOutcome::Created | ForecastEvidenceOutcome::Duplicate,
                ) => match archive.append_committed(slot.due_key(), prepared) {
                    Ok(ForecastArchiveAppendOutcome::GapPersistenceRefused) => {
                        ForecastScheduleSettlement::ArchiveGapUnrecorded
                    }
                    Ok(ForecastArchiveAppendOutcome::Settled) | Err(_) => {
                        ForecastScheduleSettlement::Evidence(evidence)
                    }
                },
                _ => ForecastScheduleSettlement::Evidence(evidence),
            };
            CompletedForecastScheduleCapture { slot, settlement }
        },
    );
    CompletedForecastResponseCapture {
        evidence,
        scheduled,
    }
}
