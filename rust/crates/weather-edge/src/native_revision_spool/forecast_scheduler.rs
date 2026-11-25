//! Coordinate one finite scheduler tick through native restart state and forecast settlement.

use std::path::PathBuf;
use std::time::Duration;

use crate::forecast_response_capture::{
    AdmittedForecastScheduleCapture, ForecastCaptureChannel, ForecastEvidenceOutcome,
    ForecastScheduleSettlement,
};
use crate::forecast_scheduler_due::{
    ForecastSchedulerCalendarPort, ForecastSchedulerDue, ForecastSchedulerDueError,
    ForecastSchedulerDueState, current_forecast_scheduler_due, missed_forecast_scheduler_due_keys,
};
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec, instant,
};

use super::{
    FORECAST_SCHEDULER_HISTORY_LIMIT, ForecastAllocationStatfsPort, ForecastSchedulerAdmission,
    ForecastSchedulerAttempt, ForecastSchedulerConfiguration, ForecastSchedulerError,
    ForecastSchedulerGap, ForecastSchedulerGapReason, ForecastSchedulerSlotsPort,
    ForecastSchedulerState, ForecastSchedulerStateCodec, ForecastSchedulerStateStoreError,
    NativeForecastSchedulerStateStore, evaluate_forecast_scheduler_admission,
};

/// Exact interval consumed by the runtime's independent, unreferenced polling lane.
pub const FORECAST_SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(30);
/// Only the finite scheduled forecast route, independent of public input.
pub const FORECAST_SCHEDULER_CAPTURE_PATH: &str = "/api/v1/sites/ballydidean/forecast?days=10";

/// One private in-process scheduled request associated with its original due key.
pub struct ForecastSchedulerCaptureRequest {
    pub slot: AdmittedForecastScheduleCapture,
    pub commit_at: String,
    pub channel: ForecastCaptureChannel,
    pub path: &'static str,
}

/// Owning request lane returns only after response and evidence/archive settlement.
pub trait ForecastSchedulerTriggerPort {
    /// Issue the admitted internal capture and retain a bounded failure category.
    fn trigger_forecast_capture(
        &mut self,
        request: ForecastSchedulerCaptureRequest,
    ) -> Result<ForecastScheduleSettlement, Option<String>>;
}

/// Independently owned existing runtime ports for a single serialized scheduler.
pub struct ForecastSchedulerPorts<'a, Codec, Calendar, Clock, Slots, Statfs, Trigger> {
    pub codec: &'a Codec,
    pub calendar: &'a Calendar,
    pub clock: &'a mut Clock,
    pub slots: &'a mut Slots,
    pub filesystem: &'a mut Statfs,
    pub trigger: &'a mut Trigger,
}

/// Result of a single tick, with any durable capture identity attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForecastSchedulerTick {
    Inactive(ForecastSchedulerAdmission),
    StateUnavailable,
    ClockRollback,
    Waiting,
    CurrentAlreadyProcessed {
        due_key: String,
    },
    Captured {
        attempt: ForecastSchedulerAttempt,
        scheduled_at: String,
    },
    CaptureFailed {
        due_key: String,
    },
}

impl ForecastSchedulerTick {
    /// expose the same bounded status labels as the source runtime.
    pub fn status(&self) -> &str {
        match self {
            Self::Inactive(admission) => admission.reason(),
            Self::StateUnavailable => "state_unavailable",
            Self::ClockRollback => "clock_rollback",
            Self::Waiting => "waiting",
            Self::CurrentAlreadyProcessed { .. } => "current_already_processed",
            Self::Captured { attempt, .. } => &attempt.status,
            Self::CaptureFailed { .. } => "capture_failed",
        }
    }
}

/// Failures contained by the independent polling owner, never by a public request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastSchedulerTickError {
    Clock,
    Due(ForecastSchedulerDueError),
    HistoryCapacity,
    State(ForecastSchedulerStateStoreError),
}

/// Single scheduler owner; a mutable borrow enforces no overlapping ticks.
pub struct NativeForecastScheduler {
    configuration: ForecastSchedulerConfiguration,
    storage: Option<NativeForecastSchedulerStateStore>,
    admission: Option<ForecastSchedulerAdmission>,
    state: ForecastSchedulerState,
    polling: bool,
}

impl NativeForecastScheduler {
    /// Open and recover the existing store without making forecast serving depend on it.
    pub fn initialize<Codec, Calendar, Clock, Slots, Statfs, Trigger>(
        root: PathBuf,
        configuration: ForecastSchedulerConfiguration,
        ports: &mut ForecastSchedulerPorts<'_, Codec, Calendar, Clock, Slots, Statfs, Trigger>,
    ) -> Self
    where
        Codec: RevisionArchiveCanonicalCodec + ForecastSchedulerStateCodec,
        Clock: RevisionArchiveCanonicalClock,
        Slots: ForecastSchedulerSlotsPort,
        Statfs: ForecastAllocationStatfsPort,
    {
        let mut value = Self {
            configuration,
            storage: NativeForecastSchedulerStateStore::initialize(root).ok(),
            admission: None,
            state: ForecastSchedulerState::empty(),
            polling: false,
        };
        let recovered = value.recover(ports.codec, ports.clock, ports.slots, ports.filesystem);
        // contain failed initialization including failed reads and writeback
        if recovered.is_err() {
            value.storage = None;
            value.admission = None;
            value.state = ForecastSchedulerState {
                activation: "state_unavailable".to_owned(),
                updated_at: ports.clock.canonical_now(),
                ..ForecastSchedulerState::empty()
            };
        }

        value
    }

    /// Borrow the exact retained health projection without cloning its history.
    pub const fn status(&self) -> &ForecastSchedulerState {
        &self.state
    }

    /// Enter polling only after active initialization; caller owns the immediate and recurring ticks.
    pub fn start_polling(&mut self) -> bool {
        // don't install a second interval or a timer for an inactive service
        if self.polling
            || !matches!(
                self.admission,
                Some(ForecastSchedulerAdmission::Active { .. })
            )
        {
            return false;
        }
        self.polling = true;
        true
    }

    /// Tell the owning runtime whether its independent interval remains installed.
    pub const fn is_polling(&self) -> bool {
        self.polling
    }

    /// Relinquish only this scheduler's process-local polling.
    pub const fn stop_polling(&mut self) {
        self.polling = false;
    }

    /// Refresh admission and issue at most one current or current catch-up forecast.
    pub fn run_due<Codec, Calendar, Clock, Slots, Statfs, Trigger>(
        &mut self,
        ports: &mut ForecastSchedulerPorts<'_, Codec, Calendar, Clock, Slots, Statfs, Trigger>,
    ) -> Result<ForecastSchedulerTick, ForecastSchedulerTickError>
    where
        Codec: RevisionArchiveCanonicalCodec + ForecastSchedulerStateCodec,
        Calendar: ForecastSchedulerCalendarPort,
        Clock: RevisionArchiveCanonicalClock,
        Slots: ForecastSchedulerSlotsPort,
        Statfs: ForecastAllocationStatfsPort,
        Trigger: ForecastSchedulerTriggerPort,
    {
        use ForecastSchedulerTick as Tick;
        use ForecastSchedulerTickError as Error;

        let Some(storage) = self.storage.as_ref() else {
            return Ok(Tick::StateUnavailable);
        };
        let now_at = ports.clock.canonical_now().ok_or(Error::Clock)?;
        let now = instant(ports.codec, &now_at).ok_or(Error::Clock)?;
        let admission = evaluate_forecast_scheduler_admission(
            self.configuration,
            Some(now),
            storage.root(),
            ports.slots,
            ports.filesystem,
            ports.codec,
        );
        self.admission = Some(admission.clone());

        let ForecastSchedulerAdmission::Active { horizon, .. } = admission else {
            // persist only the terminal horizon transition before stopping its timer
            if matches!(admission, ForecastSchedulerAdmission::CaptureEnded(_))
                && self.state.activation != "capture_ended"
            {
                self.state.activation = admission.reason().to_owned();
                self.note_checked(now_at);
                self.write(ports.codec)?;
                self.stop_polling();
            }
            return Ok(Tick::Inactive(admission));
        };

        // refuse wall-clock rollback before calculating slots or issuing any traffic
        if self
            .state
            .last_checked_at
            .as_deref()
            .and_then(|value| instant(ports.codec, value))
            .is_some_and(|previous| now < previous)
        {
            return Ok(Tick::ClockRollback);
        }
        let due = current_forecast_scheduler_due(
            &now_at,
            &horizon.horizon_end_at,
            ports.codec,
            ports.calendar,
        )
        .map_err(Error::Due)?;
        let ForecastSchedulerDueState::Current(due) = due else {
            return Ok(Tick::Waiting);
        };

        // recognize every previously attempted due key across restarts
        if self
            .state
            .attempts
            .iter()
            .any(|attempt| attempt.due_key == due.due_key)
        {
            return Ok(Tick::CurrentAlreadyProcessed {
                due_key: due.due_key,
            });
        }

        let missed = missed_forecast_scheduler_due_keys(
            self.state.last_checked_at.as_deref(),
            &now_at,
            &due.due_key,
            &horizon.horizon_end_at,
            ports.codec,
            ports.calendar,
        )
        .map_err(Error::Due)?;
        require_history(self.state.gaps.len(), missed.len())?;
        // retain each previously elapsed key without issuing backdated requests
        self.state
            .gaps
            .extend(missed.into_iter().map(|due_key| ForecastSchedulerGap {
                due_key,
                occurred_at: now_at.clone(),
                reason: ForecastSchedulerGapReason::MissedCapture,
            }));

        let result = ports
            .trigger
            .trigger_forecast_capture(ForecastSchedulerCaptureRequest {
                slot: AdmittedForecastScheduleCapture::admitted(&due.due_key),
                commit_at: now_at.clone(),
                channel: ForecastCaptureChannel::SchedulerRequest,
                path: FORECAST_SCHEDULER_CAPTURE_PATH,
            });
        self.record_result(result, due, now_at, ports.codec)
    }

    /// Restore durable state and publish the newly sampled admission label.
    fn recover(
        &mut self,
        codec: &(impl RevisionArchiveCanonicalCodec + ForecastSchedulerStateCodec),
        clock: &mut impl RevisionArchiveCanonicalClock,
        slots: &mut impl ForecastSchedulerSlotsPort,
        filesystem: &mut impl ForecastAllocationStatfsPort,
    ) -> Result<(), ForecastSchedulerTickError> {
        use ForecastSchedulerTickError as Error;

        let Some(storage) = self.storage.as_ref() else {
            return Err(Error::State(ForecastSchedulerStateStoreError::Storage));
        };
        self.state = storage.read(codec).map_err(Error::State)?;
        let now = clock.canonical_now();
        let admission = evaluate_forecast_scheduler_admission(
            self.configuration,
            now.as_deref().and_then(|value| instant(codec, value)),
            storage.root(),
            slots,
            filesystem,
            codec,
        );
        self.state.activation = admission.reason().to_owned();
        self.state.updated_at = Some(clock.canonical_now().ok_or(Error::Clock)?);
        self.admission = Some(admission);
        self.write(codec)
    }

    /// Record the one successful settlement or source-compatible failure plus gap.
    fn record_result(
        &mut self,
        result: Result<ForecastScheduleSettlement, Option<String>>,
        due: ForecastSchedulerDue,
        now_at: String,
        codec: &impl ForecastSchedulerStateCodec,
    ) -> Result<ForecastSchedulerTick, ForecastSchedulerTickError> {
        let failure = match result {
            Ok(ForecastScheduleSettlement::Evidence(
                evidence @ (ForecastEvidenceOutcome::Created | ForecastEvidenceOutcome::Duplicate),
            )) => {
                let attempt = ForecastSchedulerAttempt {
                    committed_at: now_at.clone(),
                    due_key: due.due_key.clone(),
                    status: evidence.wire_value().to_owned(),
                };
                // preserve the source catch path if the successful attempt cannot be persisted
                if require_history(self.state.attempts.len(), 1).is_ok() {
                    self.state.attempts.push(attempt.clone());
                    self.state.activation = "active".to_owned();
                    self.note_checked(now_at.clone());
                    // return only after the durable write completes
                    if self.write(codec).is_ok() {
                        return Ok(ForecastSchedulerTick::Captured {
                            attempt,
                            scheduled_at: due.scheduled_at,
                        });
                    }
                }
                None
            }
            Ok(_) => Some("capture_not_committed".to_owned()),
            Err(error) => error.filter(|code| is_error_code(code)),
        };
        require_history(self.state.attempts.len(), 1)?;
        require_history(self.state.errors.len(), 1)?;
        require_history(self.state.gaps.len(), 1)?;
        self.state.attempts.push(ForecastSchedulerAttempt {
            committed_at: now_at.clone(),
            due_key: due.due_key.clone(),
            status: "capture_failed".to_owned(),
        });
        self.state.errors.push(ForecastSchedulerError {
            at: now_at.clone(),
            code: failure.unwrap_or_else(|| "capture_failed".to_owned()),
            due_key: due.due_key.clone(),
        });
        self.state.gaps.push(ForecastSchedulerGap {
            occurred_at: now_at.clone(),
            due_key: due.due_key.clone(),
            reason: ForecastSchedulerGapReason::CaptureFailed,
        });
        self.note_checked(now_at);
        self.write(codec)?;
        Ok(ForecastSchedulerTick::CaptureFailed {
            due_key: due.due_key,
        })
    }

    /// Keep both elapsed-observation fields in step after a terminal action.
    fn note_checked(&mut self, now_at: String) {
        self.state.last_checked_at = Some(now_at.clone());
        self.state.updated_at = Some(now_at);
    }

    /// Save with the native two-generation write protocol.
    fn write(
        &mut self,
        codec: &impl ForecastSchedulerStateCodec,
    ) -> Result<(), ForecastSchedulerTickError> {
        use ForecastSchedulerTickError as Error;
        self.storage
            .as_mut()
            .ok_or(Error::State(ForecastSchedulerStateStoreError::Storage))?
            .write(&self.state, codec)
            .map_err(Error::State)
    }
}

/// Stop at the source history bound instead of dropping accepted causal history.
fn require_history(count: usize, extra: usize) -> Result<(), ForecastSchedulerTickError> {
    // protect the bound without relying on arithmetic wrapping
    if extra > FORECAST_SCHEDULER_HISTORY_LIMIT.saturating_sub(count) {
        return Err(ForecastSchedulerTickError::HistoryCapacity);
    }
    Ok(())
}

/// Keep a trigger's categorical error only when it has the exact source spelling.
fn is_error_code(code: &str) -> bool {
    (1..=64).contains(&code.len())
        && code
            .bytes()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == b'_')
}
