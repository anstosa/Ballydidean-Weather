use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

pub const WIND_MOVING_BLOCK_DAYS: usize = 7;
pub const WIND_MOVING_BLOCK_LOWER_INDEX: usize = 49;
pub const WIND_MOVING_BLOCK_REPLICATES: usize = 2_000;
pub const WIND_MOVING_BLOCK_SEED: u32 = 0x5eed_b007;
pub const WIND_MOVING_BLOCK_UPPER_INDEX: usize = 1_949;

/// One paired forecast occurrence retained in original event order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindBootstrapPairedEvent {
    pub actual: f64,
    pub adjusted_prediction: f64,
    pub raw_prediction: f64,
}

/// One admitted local-date slot, including an explicitly empty slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindBootstrapDateSlot<'a> {
    pub events: &'a [WindBootstrapPairedEvent],
    pub local_day_ordinal: i64,
}

/// One scored development window in caller-provided chronological order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindBootstrapScoredWindow<'a> {
    pub date_slots: &'a [WindBootstrapDateSlot<'a>],
    pub key: &'a str,
}

/// Replicate-outer, window-inner non-circular block starts.
pub type WindMovingBlockStartPlan = Vec<Vec<Vec<usize>>>;

/// Exact paired moving-block bootstrap evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct WindMovingBlockBootstrapResult {
    pub adjusted_loss: f64,
    pub bootstrap_lower_bound: f64,
    pub bootstrap_upper_bound: f64,
    pub event_count: usize,
    pub raw_loss: f64,
    pub replicate_skills: Vec<f64>,
    pub skill: f64,
}

/// Failure to admit or score one moving-block bootstrap input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindMovingBlockBootstrapError {
    DuplicateWindowKey,
    EmptyWindowKey,
    InvalidBlockPlan,
    InvalidBlockStart,
    InvalidEvent,
    InvalidReplicateSkill,
    NoWindows,
    NonConsecutiveDateSlots,
    WindowTooShort,
    ZeroMatchedEvents,
}

impl Display for WindMovingBlockBootstrapError {
    /// Render a stable failure without exposing retained development values.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // keep each source failure distinguishable for the development owner
        match self {
            Self::DuplicateWindowKey => formatter.write_str("bootstrap window key is duplicated"),
            Self::EmptyWindowKey => formatter.write_str("bootstrap window key is empty"),
            Self::InvalidBlockPlan => formatter.write_str("bootstrap block plan is invalid"),
            Self::InvalidBlockStart => formatter.write_str("bootstrap block start is invalid"),
            Self::InvalidEvent => formatter.write_str("bootstrap paired event is invalid"),
            Self::InvalidReplicateSkill => {
                formatter.write_str("bootstrap replicate skill is invalid")
            }
            Self::NoWindows => formatter.write_str("bootstrap requires a scored window"),
            Self::NonConsecutiveDateSlots => {
                formatter.write_str("bootstrap date slots are not consecutive")
            }
            Self::WindowTooShort => {
                formatter.write_str("bootstrap scored window has fewer than seven dates")
            }
            Self::ZeroMatchedEvents => {
                formatter.write_str("bootstrap replicate contains zero matched events")
            }
        }
    }
}

impl Error for WindMovingBlockBootstrapError {}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PairedLoss {
    adjusted_loss: f64,
    event_count: usize,
    raw_loss: f64,
    skill: f64,
}

/// Advance the frozen unsigned xorshift32 state once.
pub const fn wind_xorshift32(state: u32) -> u32 {
    let next = state ^ state.wrapping_shl(13);
    let next = next ^ next.wrapping_shr(17);
    next ^ next.wrapping_shl(5)
}

/// Create the exact replicate-outer, window-inner block-start plan.
pub fn create_wind_moving_block_start_plan(
    window_lengths: &[usize],
) -> Result<WindMovingBlockStartPlan, WindMovingBlockBootstrapError> {
    validate_window_lengths(window_lengths)?;
    let mut state = WIND_MOVING_BLOCK_SEED;
    let mut plan = Vec::with_capacity(WIND_MOVING_BLOCK_REPLICATES);

    // advance one continuous stream through every replicate and window
    for _replicate in 0..WIND_MOVING_BLOCK_REPLICATES {
        let mut replicate_plan = Vec::with_capacity(window_lengths.len());

        // process development windows in caller-provided chronological order
        for &length in window_lengths {
            let mut starts = Vec::with_capacity(length.div_ceil(WIND_MOVING_BLOCK_DAYS));
            let mut sampled_slots = 0usize;

            // draw whole blocks until the requested window is covered
            while sampled_slots < length {
                state = wind_xorshift32(state);
                let uniform = f64::from(state) / 4_294_967_296.0;
                let start_count = length - WIND_MOVING_BLOCK_DAYS + 1;
                starts.push((uniform * start_count as f64).floor() as usize);
                sampled_slots += WIND_MOVING_BLOCK_DAYS;
            }

            replicate_plan.push(starts);
        }

        plan.push(replicate_plan);
    }

    Ok(plan)
}

/// Expand non-circular block starts and truncate them to one window length.
pub fn expand_wind_non_circular_block_starts(
    starts: &[usize],
    window_length: usize,
) -> Result<Vec<usize>, WindMovingBlockBootstrapError> {
    validate_window_lengths(&[window_length])?;
    let required_starts = window_length.div_ceil(WIND_MOVING_BLOCK_DAYS);

    // require the literal stop-after-coverage draw count
    if starts.len() != required_starts {
        return Err(WindMovingBlockBootstrapError::InvalidBlockPlan);
    }

    let maximum_start = window_length - WIND_MOVING_BLOCK_DAYS;
    let mut offsets = Vec::with_capacity(required_starts * WIND_MOVING_BLOCK_DAYS);

    // expand every block without crossing the window boundary
    for &start in starts {
        // reject a start outside zero through N minus seven
        if start > maximum_start {
            return Err(WindMovingBlockBootstrapError::InvalidBlockStart);
        }

        // append the exact seven consecutive date slots
        for offset in 0..WIND_MOVING_BLOCK_DAYS {
            offsets.push(start + offset);
        }
    }

    offsets.truncate(window_length);
    Ok(offsets)
}

/// Compute one scalar or circular paired event loss.
pub fn wind_bootstrap_event_loss(
    actual: f64,
    prediction: f64,
    direction: bool,
) -> Result<f64, WindMovingBlockBootstrapError> {
    // reject nonfinite score inputs before subtraction
    if !actual.is_finite() || !prediction.is_finite() {
        return Err(WindMovingBlockBootstrapError::InvalidEvent);
    }

    let difference = actual - prediction;

    // preserve the circular primitive's finite-angle rejection
    if direction && !difference.is_finite() {
        return Err(WindMovingBlockBootstrapError::InvalidEvent);
    }

    Ok(if direction {
        wrap_180(difference).abs()
    } else {
        difference.abs()
    })
}

/// Score a supplied frozen plan without changing its draw order.
pub fn score_wind_moving_block_bootstrap_plan(
    windows: &[WindBootstrapScoredWindow<'_>],
    plan: &WindMovingBlockStartPlan,
    direction: bool,
) -> Result<WindMovingBlockBootstrapResult, WindMovingBlockBootstrapError> {
    validate_scored_windows(windows)?;

    // require the exact frozen replicate count
    if plan.len() != WIND_MOVING_BLOCK_REPLICATES {
        return Err(WindMovingBlockBootstrapError::InvalidBlockPlan);
    }

    let original = aggregate_original_occurrences(windows, direction)?;
    let mut replicate_skills = Vec::with_capacity(WIND_MOVING_BLOCK_REPLICATES);

    // score every replicate without dropping an empty resample
    for replicate_plan in plan {
        // require one block-start list for every scored window
        if replicate_plan.len() != windows.len() {
            return Err(WindMovingBlockBootstrapError::InvalidBlockPlan);
        }

        let replicate = aggregate_resampled_occurrences(windows, replicate_plan, direction)?;

        // retain only finite skills exactly like the source contract
        if !replicate.skill.is_finite() {
            return Err(WindMovingBlockBootstrapError::InvalidReplicateSkill);
        }

        replicate_skills.push(replicate.skill);
    }

    let mut sorted_skills = replicate_skills.clone();
    sorted_skills
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));

    Ok(WindMovingBlockBootstrapResult {
        adjusted_loss: original.adjusted_loss,
        bootstrap_lower_bound: sorted_skills[WIND_MOVING_BLOCK_LOWER_INDEX],
        bootstrap_upper_bound: sorted_skills[WIND_MOVING_BLOCK_UPPER_INDEX],
        event_count: original.event_count,
        raw_loss: original.raw_loss,
        replicate_skills,
        skill: original.skill,
    })
}

/// Generate and score one independently reset frozen bootstrap plan.
pub fn score_wind_moving_block_bootstrap(
    windows: &[WindBootstrapScoredWindow<'_>],
    direction: bool,
) -> Result<WindMovingBlockBootstrapResult, WindMovingBlockBootstrapError> {
    validate_scored_windows(windows)?;
    let lengths = windows
        .iter()
        .map(|window| window.date_slots.len())
        .collect::<Vec<_>>();
    let plan = create_wind_moving_block_start_plan(&lengths)?;
    score_wind_moving_block_bootstrap_plan(windows, &plan, direction)
}

/// Compute the frozen paired skill including both zero-baseline cases.
pub(crate) fn wind_paired_skill(
    raw_loss: f64,
    adjusted_loss: f64,
) -> Result<f64, WindMovingBlockBootstrapError> {
    // reject nonfinite or negative aggregate losses
    if !raw_loss.is_finite() || !adjusted_loss.is_finite() || raw_loss < 0.0 || adjusted_loss < 0.0
    {
        return Err(WindMovingBlockBootstrapError::InvalidEvent);
    }

    // preserve the no-demonstrated-gain zero case
    if raw_loss == 0.0 && adjusted_loss == 0.0 {
        return Ok(0.0);
    }

    // freeze the harmed zero-baseline case
    if raw_loss == 0.0 {
        return Ok(-1.0);
    }

    Ok((raw_loss - adjusted_loss) / raw_loss)
}

/// Validate all window lengths before the first pseudo-random draw.
fn validate_window_lengths(window_lengths: &[usize]) -> Result<(), WindMovingBlockBootstrapError> {
    // require at least one scored window
    if window_lengths.is_empty() {
        return Err(WindMovingBlockBootstrapError::NoWindows);
    }

    // reject undersized windows before creating sampler state
    for &length in window_lengths {
        // enforce whole non-circular seven-date blocks
        if length < WIND_MOVING_BLOCK_DAYS {
            return Err(WindMovingBlockBootstrapError::WindowTooShort);
        }
    }

    Ok(())
}

/// Validate unique windows and their admitted consecutive local-day ordinals.
fn validate_scored_windows(
    windows: &[WindBootstrapScoredWindow<'_>],
) -> Result<(), WindMovingBlockBootstrapError> {
    let lengths = windows
        .iter()
        .map(|window| window.date_slots.len())
        .collect::<Vec<_>>();
    validate_window_lengths(&lengths)?;
    let mut keys = BTreeSet::<&str>::new();

    // inspect every scored window before sampling
    for window in windows {
        // reject an empty fold identity
        if window.key.is_empty() {
            return Err(WindMovingBlockBootstrapError::EmptyWindowKey);
        }

        // reject a repeated fold identity
        if !keys.insert(window.key) {
            return Err(WindMovingBlockBootstrapError::DuplicateWindowKey);
        }

        let first = window.date_slots[0].local_day_ordinal;

        // require every retained date slot to follow its admitted ordinal
        for (index, slot) in window.date_slots.iter().enumerate() {
            let Ok(offset) = i64::try_from(index) else {
                return Err(WindMovingBlockBootstrapError::NonConsecutiveDateSlots);
            };
            let Some(expected) = first.checked_add(offset) else {
                return Err(WindMovingBlockBootstrapError::NonConsecutiveDateSlots);
            };

            // reject a shifted, duplicated, or missing local date
            if slot.local_day_ordinal != expected {
                return Err(WindMovingBlockBootstrapError::NonConsecutiveDateSlots);
            }
        }
    }

    Ok(())
}

/// Aggregate original occurrences in window, date, then event order.
fn aggregate_original_occurrences(
    windows: &[WindBootstrapScoredWindow<'_>],
    direction: bool,
) -> Result<PairedLoss, WindMovingBlockBootstrapError> {
    let mut accumulator = PairedLossAccumulator::default();

    // preserve caller-provided window order
    for window in windows {
        // preserve explicit local-date slot order including empty slots
        for slot in window.date_slots {
            accumulator.add_events(slot.events, direction)?;
        }
    }

    accumulator.finish()
}

/// Aggregate resampled occurrences without materializing duplicate event rows.
fn aggregate_resampled_occurrences(
    windows: &[WindBootstrapScoredWindow<'_>],
    replicate_plan: &[Vec<usize>],
    direction: bool,
) -> Result<PairedLoss, WindMovingBlockBootstrapError> {
    let mut accumulator = PairedLossAccumulator::default();

    // resample each window independently without crossing its boundary
    for (window, starts) in windows.iter().zip(replicate_plan) {
        let offsets = expand_wind_non_circular_block_starts(starts, window.date_slots.len())?;

        // retain repeated and empty selected date slots exactly
        for offset in offsets {
            accumulator.add_events(window.date_slots[offset].events, direction)?;
        }
    }

    accumulator.finish()
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct PairedLossAccumulator {
    adjusted_loss: f64,
    event_count: usize,
    raw_loss: f64,
}

impl PairedLossAccumulator {
    /// Append exact event occurrences without reassociating their losses.
    fn add_events(
        &mut self,
        events: &[WindBootstrapPairedEvent],
        direction: bool,
    ) -> Result<(), WindMovingBlockBootstrapError> {
        // score the raw then adjusted loss for every paired occurrence
        for event in events {
            self.raw_loss +=
                wind_bootstrap_event_loss(event.actual, event.raw_prediction, direction)?;
            self.adjusted_loss +=
                wind_bootstrap_event_loss(event.actual, event.adjusted_prediction, direction)?;
            self.event_count = self
                .event_count
                .checked_add(1)
                .ok_or(WindMovingBlockBootstrapError::InvalidEvent)?;
        }

        Ok(())
    }

    /// Finish the paired mean losses and frozen skill calculation.
    fn finish(self) -> Result<PairedLoss, WindMovingBlockBootstrapError> {
        // reject zero-event replicates rather than dropping them
        if self.event_count == 0 {
            return Err(WindMovingBlockBootstrapError::ZeroMatchedEvents);
        }

        let count = self.event_count as f64;
        let raw_loss = self.raw_loss / count;
        let adjusted_loss = self.adjusted_loss / count;
        let skill = wind_paired_skill(raw_loss, adjusted_loss)?;
        Ok(PairedLoss {
            adjusted_loss,
            event_count: self.event_count,
            raw_loss,
            skill,
        })
    }
}

/// Wrap one finite angular residual into the signed half-open circle.
fn wrap_180(value: f64) -> f64 {
    let wrapped = (((value + 180.0) % 360.0) + 360.0) % 360.0 - 180.0;

    // normalize negative zero like the source primitive
    if wrapped == 0.0 { 0.0 } else { wrapped }
}
