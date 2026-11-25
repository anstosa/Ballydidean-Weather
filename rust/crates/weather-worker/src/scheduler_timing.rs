use std::error::Error;
use std::fmt::{Display, Formatter};

pub const MAXIMUM_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const MAXIMUM_SCHEDULE_JITTER_MS: u64 = 30_000;
pub const MINIMUM_JAVASCRIPT_DATE_MS: i64 = -8_640_000_000_000_000;
pub const MAXIMUM_JAVASCRIPT_DATE_MS: i64 = 8_640_000_000_000_000;

/// Bounded deterministic scheduler timing failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerSchedulerTimingError {
    InvalidCadence,
    InvalidNow,
    ArithmeticOverflow,
    ScheduledTimeOutsideDateRange,
}

impl Display for WorkerSchedulerTimingError {
    /// Render stable scheduler admission and arithmetic failures.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCadence => formatter.write_str("scheduler cadence must be positive"),
            Self::InvalidNow => {
                formatter.write_str("scheduler clock is outside the JavaScript Date range")
            }
            Self::ArithmeticOverflow => formatter.write_str("scheduler time arithmetic overflowed"),
            Self::ScheduledTimeOutsideDateRange => {
                formatter.write_str("scheduled time is outside the JavaScript Date range")
            }
        }
    }
}

impl Error for WorkerSchedulerTimingError {}

/// Calculate the next stable anchored slot from an approved SHA-256 digest prefix.
pub fn next_worker_scheduled_at_ms(
    now_ms: i64,
    cadence_ms: u64,
    digest_prefix: [u8; 4],
) -> Result<i64, WorkerSchedulerTimingError> {
    // mirror Number.isSafeInteger and the positive cadence requirement
    if cadence_ms == 0 || cadence_ms > MAXIMUM_JAVASCRIPT_SAFE_INTEGER {
        return Err(WorkerSchedulerTimingError::InvalidCadence);
    }
    // reject clocks that could not have been represented by the Node Date input
    if !(MINIMUM_JAVASCRIPT_DATE_MS..=MAXIMUM_JAVASCRIPT_DATE_MS).contains(&now_ms) {
        return Err(WorkerSchedulerTimingError::InvalidNow);
    }

    let cadence =
        i64::try_from(cadence_ms).map_err(|_| WorkerSchedulerTimingError::ArithmeticOverflow)?;
    let maximum_jitter = MAXIMUM_SCHEDULE_JITTER_MS.min(cadence_ms - 1);
    let jitter_range = maximum_jitter + 1;
    let jitter = u64::from(u32::from_be_bytes(digest_prefix)) % jitter_range;
    let current_cycle = now_ms
        .div_euclid(cadence)
        .checked_mul(cadence)
        .ok_or(WorkerSchedulerTimingError::ArithmeticOverflow)?;
    let mut candidate = current_cycle
        .checked_add(
            i64::try_from(jitter).map_err(|_| WorkerSchedulerTimingError::ArithmeticOverflow)?,
        )
        .ok_or(WorkerSchedulerTimingError::ArithmeticOverflow)?;

    // move past an elapsed or exactly-current anchored slot
    if candidate <= now_ms {
        candidate = candidate
            .checked_add(cadence)
            .ok_or(WorkerSchedulerTimingError::ArithmeticOverflow)?;
    }
    // do not construct the invalid Date that Node would otherwise return at the boundary
    if !(MINIMUM_JAVASCRIPT_DATE_MS..=MAXIMUM_JAVASCRIPT_DATE_MS).contains(&candidate) {
        return Err(WorkerSchedulerTimingError::ScheduledTimeOutsideDateRange);
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Match the first four SHA-256 bytes and anchored time for Node's worker-a fixture.
    #[test]
    fn matches_worker_a_node_fixture() {
        assert_eq!(
            next_worker_scheduled_at_ms(1_787_375_220_000, 60_000, [106, 101, 226, 55]),
            Ok(1_787_375_222_443)
        );
    }

    /// Move an exact zero-jitter slot strictly into the following cycle.
    #[test]
    fn advances_strictly_after_an_exact_slot() {
        assert_eq!(
            next_worker_scheduled_at_ms(120_000, 60_000, [0, 0, 0, 0]),
            Ok(180_000)
        );
    }

    /// Use Euclidean floor rather than truncation for clocks before the epoch.
    #[test]
    fn anchors_negative_milliseconds_with_euclidean_floor() {
        assert_eq!(next_worker_scheduled_at_ms(-1, 60_000, [0, 0, 0, 0]), Ok(0));
    }

    /// Keep jitter inside tiny cadence intervals, including a one-millisecond cadence.
    #[test]
    fn bounds_jitter_for_small_cadences() {
        assert_eq!(
            next_worker_scheduled_at_ms(0, 2, [255, 255, 255, 255]),
            Ok(1)
        );
        assert_eq!(
            next_worker_scheduled_at_ms(0, 1, [255, 255, 255, 255]),
            Ok(1)
        );
    }

    /// Reject invalid cadence and Date-range overflow without wrapping time arithmetic.
    #[test]
    fn rejects_invalid_or_unrepresentable_times() {
        assert_eq!(
            next_worker_scheduled_at_ms(0, 0, [0, 0, 0, 0]),
            Err(WorkerSchedulerTimingError::InvalidCadence)
        );
        assert_eq!(
            next_worker_scheduled_at_ms(0, MAXIMUM_JAVASCRIPT_SAFE_INTEGER + 1, [0, 0, 0, 0]),
            Err(WorkerSchedulerTimingError::InvalidCadence)
        );
        assert_eq!(
            next_worker_scheduled_at_ms(MAXIMUM_JAVASCRIPT_DATE_MS, 1, [0, 0, 0, 0]),
            Err(WorkerSchedulerTimingError::ScheduledTimeOutsideDateRange)
        );
        assert_eq!(
            next_worker_scheduled_at_ms(MAXIMUM_JAVASCRIPT_DATE_MS + 1, 60_000, [0, 0, 0, 0]),
            Err(WorkerSchedulerTimingError::InvalidNow)
        );
    }
}
