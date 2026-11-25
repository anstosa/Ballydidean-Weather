//! Deterministic UTC due selection for retained forecast-evidence capture.

use std::collections::BTreeMap;

use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant};

/// First authenticated instant admitted by the Node forecast scheduler.
pub const FORECAST_SCHEDULER_CAPTURE_START: &str = "2026-10-08T00:00:00.000Z";

const CATCHUP_MILLISECONDS: i64 = 12 * 60 * 60 * 1_000;
const UTC_DAY_MILLISECONDS: i64 = 24 * 60 * 60 * 1_000;
const BASE_OFFSETS_MILLISECONDS: [i64; 4] = [
    35 * 60 * 1_000,
    (6 * 60 + 35) * 60 * 1_000,
    (12 * 60 + 35) * 60 * 1_000,
    (18 * 60 + 35) * 60 * 1_000,
];

/// Closed UTC date offsets used by the two-day candidate window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastSchedulerDayOffset {
    Yesterday,
    Today,
}

/// One runtime-resolved canonical base instant on a fixed UTC schedule day.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastSchedulerUtcBase {
    pub canonical_at: String,
    pub epoch_milliseconds: i64,
}

/// Four runtime-resolved 00/06/12/18:35 bases for one UTC date.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastSchedulerUtcDay {
    pub bases: [ForecastSchedulerUtcBase; 4],
    pub midnight_epoch_milliseconds: i64,
}

/// Approved UTC calendar and formatter boundary without a Rust date implementation.
pub trait ForecastSchedulerCalendarPort {
    /// Resolve yesterday or today relative to one admitted observation instant.
    fn fixed_utc_day(
        &self,
        observation_epoch_milliseconds: i64,
        offset: ForecastSchedulerDayOffset,
    ) -> Option<ForecastSchedulerUtcDay>;

    /// Format one runtime-computed instant with the canonical UTC owner.
    fn canonical_utc(&self, epoch_milliseconds: i64) -> Option<String>;

    /// Advance an observation by one exact UTC date as the source loop does.
    fn next_utc_day_observation(&self, epoch_milliseconds: i64) -> Option<i64>;
}

/// One immutable jittered scheduler cycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForecastSchedulerDue {
    pub due_key: String,
    pub jitter_seconds: u16,
    pub scheduled_at: String,
    pub scheduled_at_epoch_milliseconds: i64,
}

/// Current scheduler projection without inventing a due cycle before its jitter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForecastSchedulerDueState {
    Current(ForecastSchedulerDue),
    Waiting,
}

/// Bounded input or admitted calendar failures from due calculation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForecastSchedulerDueError {
    Calendar,
    CaptureEnd,
    LastCheckedAt,
    Now,
    Overflow,
}

/// Select the newest elapsed due cycle within the exact twelve-hour catch-up window.
pub fn current_forecast_scheduler_due(
    now_at: &str,
    capture_end_at: &str,
    codec: &impl RevisionArchiveCanonicalCodec,
    calendar: &impl ForecastSchedulerCalendarPort,
) -> Result<ForecastSchedulerDueState, ForecastSchedulerDueError> {
    let now_epoch_milliseconds = instant(codec, now_at).ok_or(ForecastSchedulerDueError::Now)?;
    let capture_end_epoch_milliseconds =
        instant(codec, capture_end_at).ok_or(ForecastSchedulerDueError::CaptureEnd)?;

    // stop at the finite authenticated horizon before consulting the calendar
    if now_epoch_milliseconds >= capture_end_epoch_milliseconds {
        return Ok(ForecastSchedulerDueState::Waiting);
    }
    let candidates = due_candidates(
        now_epoch_milliseconds,
        capture_end_epoch_milliseconds,
        codec,
        calendar,
    )?;

    // choose only the newest elapsed candidate inside the catch-up horizon
    for candidate in candidates.into_iter().rev() {
        let elapsed = now_epoch_milliseconds
            .checked_sub(candidate.scheduled_at_epoch_milliseconds)
            .ok_or(ForecastSchedulerDueError::Overflow)?;

        // preserve future jitter and reject older backlog
        if (0..=CATCHUP_MILLISECONDS).contains(&elapsed) {
            return Ok(ForecastSchedulerDueState::Current(candidate));
        }
    }
    Ok(ForecastSchedulerDueState::Waiting)
}

/// Calculate every elapsed unselected due identity after the durable observation.
pub fn missed_forecast_scheduler_due_keys(
    last_checked_at: Option<&str>,
    now_at: &str,
    selected_due_key: &str,
    capture_end_at: &str,
    codec: &impl RevisionArchiveCanonicalCodec,
    calendar: &impl ForecastSchedulerCalendarPort,
) -> Result<Vec<String>, ForecastSchedulerDueError> {
    // treat first startup as one current catch-up without invented history
    let Some(last_checked_at) = last_checked_at else {
        return Ok(Vec::new());
    };
    let previous_epoch_milliseconds =
        instant(codec, last_checked_at).ok_or(ForecastSchedulerDueError::LastCheckedAt)?;
    let now_epoch_milliseconds = instant(codec, now_at).ok_or(ForecastSchedulerDueError::Now)?;
    let capture_end_epoch_milliseconds =
        instant(codec, capture_end_at).ok_or(ForecastSchedulerDueError::CaptureEnd)?;
    let mut candidates = BTreeMap::new();
    let mut cursor = previous_epoch_milliseconds;

    // inspect each source UTC date between both durable observations
    while cursor <= now_epoch_milliseconds {
        for candidate in due_candidates(cursor, capture_end_epoch_milliseconds, codec, calendar)? {
            candidates.insert(candidate.due_key.clone(), candidate);
        }
        let next = calendar
            .next_utc_day_observation(cursor)
            .ok_or(ForecastSchedulerDueError::Calendar)?;
        let expected = cursor
            .checked_add(UTC_DAY_MILLISECONDS)
            .ok_or(ForecastSchedulerDueError::Overflow)?;

        // reject a calendar owner that changes the source's fixed UTC-day step
        if next != expected {
            return Err(ForecastSchedulerDueError::Calendar);
        }
        cursor = next;
    }
    let mut missed = candidates
        .into_values()
        .filter(
            // retain elapsed, unselected cycles strictly after the last observation
            |candidate| {
                candidate.scheduled_at_epoch_milliseconds > previous_epoch_milliseconds
                    && candidate.scheduled_at_epoch_milliseconds <= now_epoch_milliseconds
                    && candidate.due_key.as_str() != selected_due_key
            },
        )
        .collect::<Vec<_>>();
    missed.sort_by_key(|candidate| candidate.scheduled_at_epoch_milliseconds);
    Ok(missed
        .into_iter()
        .map(|candidate| candidate.due_key)
        .collect())
}

// produce yesterday and today using only the admitted calendar and codec owners
fn due_candidates(
    observation_epoch_milliseconds: i64,
    capture_end_epoch_milliseconds: i64,
    codec: &impl RevisionArchiveCanonicalCodec,
    calendar: &impl ForecastSchedulerCalendarPort,
) -> Result<Vec<ForecastSchedulerDue>, ForecastSchedulerDueError> {
    let capture_start_epoch_milliseconds = instant(codec, FORECAST_SCHEDULER_CAPTURE_START)
        .ok_or(ForecastSchedulerDueError::Calendar)?;
    let yesterday = calendar
        .fixed_utc_day(
            observation_epoch_milliseconds,
            ForecastSchedulerDayOffset::Yesterday,
        )
        .ok_or(ForecastSchedulerDueError::Calendar)?;
    let today = calendar
        .fixed_utc_day(
            observation_epoch_milliseconds,
            ForecastSchedulerDayOffset::Today,
        )
        .ok_or(ForecastSchedulerDueError::Calendar)?;
    let expected_today = yesterday
        .midnight_epoch_milliseconds
        .checked_add(UTC_DAY_MILLISECONDS)
        .ok_or(ForecastSchedulerDueError::Overflow)?;
    let tomorrow = today
        .midnight_epoch_milliseconds
        .checked_add(UTC_DAY_MILLISECONDS)
        .ok_or(ForecastSchedulerDueError::Overflow)?;

    // bind both resolved days to the observation and exact adjacent UTC dates
    if today.midnight_epoch_milliseconds != expected_today
        || observation_epoch_milliseconds < today.midnight_epoch_milliseconds
        || observation_epoch_milliseconds >= tomorrow
    {
        return Err(ForecastSchedulerDueError::Calendar);
    }
    let mut candidates = Vec::with_capacity(8);

    // retain source order from yesterday through today's four fixed bases
    for day in [yesterday, today] {
        validate_utc_day(&day, codec)?;

        for base in day.bases {
            // exclude bases outside the exact source start and finite end bounds
            if base.epoch_milliseconds < capture_start_epoch_milliseconds
                || base.epoch_milliseconds >= capture_end_epoch_milliseconds
            {
                continue;
            }
            let due_key = format!("capture/{}", base.canonical_at);
            let hash_input = format!("ballydidean|{due_key}");
            let digest = codec.sha256(hash_input.as_bytes());
            let jitter_seconds = u16::try_from(
                u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 300,
            )
            .map_err(|_| ForecastSchedulerDueError::Overflow)?;
            let jitter_milliseconds = i64::from(jitter_seconds)
                .checked_mul(1_000)
                .ok_or(ForecastSchedulerDueError::Overflow)?;
            let scheduled_at_epoch_milliseconds = base
                .epoch_milliseconds
                .checked_add(jitter_milliseconds)
                .ok_or(ForecastSchedulerDueError::Overflow)?;
            let scheduled_at = calendar
                .canonical_utc(scheduled_at_epoch_milliseconds)
                .ok_or(ForecastSchedulerDueError::Calendar)?;

            // require the formatter and parser to agree on the exact jittered instant
            if instant(codec, &scheduled_at) != Some(scheduled_at_epoch_milliseconds) {
                return Err(ForecastSchedulerDueError::Calendar);
            }
            candidates.push(ForecastSchedulerDue {
                due_key,
                jitter_seconds,
                scheduled_at,
                scheduled_at_epoch_milliseconds,
            });
        }
    }
    Ok(candidates)
}

// verify the runtime calendar returned the four exact fixed source bases
fn validate_utc_day(
    day: &ForecastSchedulerUtcDay,
    codec: &impl RevisionArchiveCanonicalCodec,
) -> Result<(), ForecastSchedulerDueError> {
    for (base, offset) in day.bases.iter().zip(BASE_OFFSETS_MILLISECONDS) {
        let expected = day
            .midnight_epoch_milliseconds
            .checked_add(offset)
            .ok_or(ForecastSchedulerDueError::Overflow)?;

        // bind the canonical spelling and fixed offset to the same admitted instant
        if base.epoch_milliseconds != expected
            || instant(codec, &base.canonical_at) != Some(expected)
        {
            return Err(ForecastSchedulerDueError::Calendar);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_7_MIDNIGHT: i64 = 1_791_331_200_000;
    const DAY_8_MIDNIGHT: i64 = 1_791_417_600_000;
    const DAY_9_MIDNIGHT: i64 = 1_791_504_000_000;
    const CAPTURE_END: &str = "2028-10-08T07:00:00.000Z";

    struct NodeWitness;

    impl RevisionArchiveCanonicalCodec for NodeWitness {
        // admit only exact Node witness instants used by this scheduler proof
        fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
            witness_milliseconds(text)
        }

        // return the independently captured Node SHA-256 prefix for each due key
        fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
            let prefix = match bytes {
                b"ballydidean|capture/2026-10-07T00:35:00.000Z" => [0x42, 0xb1, 0xef, 0xa5],
                b"ballydidean|capture/2026-10-07T06:35:00.000Z" => [0x5e, 0x7b, 0xce, 0x36],
                b"ballydidean|capture/2026-10-07T12:35:00.000Z" => [0xd9, 0x55, 0x61, 0x30],
                b"ballydidean|capture/2026-10-07T18:35:00.000Z" => [0x1b, 0x53, 0x1e, 0xe4],
                b"ballydidean|capture/2026-10-08T00:35:00.000Z" => [0x4c, 0x01, 0xbc, 0xd3],
                b"ballydidean|capture/2026-10-08T06:35:00.000Z" => [0x44, 0x87, 0xd5, 0x1a],
                b"ballydidean|capture/2026-10-08T12:35:00.000Z" => [0x10, 0xab, 0xee, 0x3c],
                b"ballydidean|capture/2026-10-08T18:35:00.000Z" => [0x64, 0xba, 0x60, 0xdc],
                b"ballydidean|capture/2026-10-09T00:35:00.000Z" => [0x77, 0x79, 0xe1, 0xbd],
                _ => panic!("unexpected scheduler hash input"),
            };
            let mut digest = [0_u8; 32];
            digest[..4].copy_from_slice(&prefix);
            digest
        }
    }

    impl ForecastSchedulerCalendarPort for NodeWitness {
        // project the exact witness date selected by the observation and offset
        fn fixed_utc_day(
            &self,
            observation_epoch_milliseconds: i64,
            offset: ForecastSchedulerDayOffset,
        ) -> Option<ForecastSchedulerUtcDay> {
            // admit only the three adjacent witness dates used by this proof
            let today = if (DAY_7_MIDNIGHT..DAY_8_MIDNIGHT)
                .contains(&observation_epoch_milliseconds)
            {
                DAY_7_MIDNIGHT
            } else if (DAY_8_MIDNIGHT..DAY_9_MIDNIGHT).contains(&observation_epoch_milliseconds) {
                DAY_8_MIDNIGHT
            } else if (DAY_9_MIDNIGHT..DAY_9_MIDNIGHT + UTC_DAY_MILLISECONDS)
                .contains(&observation_epoch_milliseconds)
            {
                DAY_9_MIDNIGHT
            } else {
                return None;
            };
            let midnight = match offset {
                ForecastSchedulerDayOffset::Yesterday => today - UTC_DAY_MILLISECONDS,
                ForecastSchedulerDayOffset::Today => today,
            };
            witness_day(midnight)
        }

        // format only the independently captured jittered witness instants
        fn canonical_utc(&self, epoch_milliseconds: i64) -> Option<String> {
            witness_canonical(epoch_milliseconds).map(str::to_owned)
        }

        // advance the source observation by one exact UTC day
        fn next_utc_day_observation(&self, epoch_milliseconds: i64) -> Option<i64> {
            epoch_milliseconds.checked_add(UTC_DAY_MILLISECONDS)
        }
    }

    #[test]
    fn selects_the_exact_node_jittered_current_cycle() {
        let due = current_forecast_scheduler_due(
            "2026-10-08T06:40:00.000Z",
            CAPTURE_END,
            &NodeWitness,
            &NodeWitness,
        )
        .expect("select current due");
        assert_eq!(
            due,
            ForecastSchedulerDueState::Current(ForecastSchedulerDue {
                due_key: "capture/2026-10-08T06:35:00.000Z".to_owned(),
                jitter_seconds: 202,
                scheduled_at: "2026-10-08T06:38:22.000Z".to_owned(),
                scheduled_at_epoch_milliseconds: 1_791_441_502_000,
            })
        );

        let before_jitter = current_forecast_scheduler_due(
            "2026-10-08T06:36:00.000Z",
            CAPTURE_END,
            &NodeWitness,
            &NodeWitness,
        )
        .expect("select preceding due");
        assert!(matches!(
            before_jitter,
            ForecastSchedulerDueState::Current(ForecastSchedulerDue { ref due_key, .. })
                if due_key == "capture/2026-10-08T00:35:00.000Z"
        ));
    }

    #[test]
    fn waits_before_the_first_bounded_jitter_and_at_the_end() {
        assert_eq!(
            current_forecast_scheduler_due(
                "2026-10-08T00:36:00.000Z",
                CAPTURE_END,
                &NodeWitness,
                &NodeWitness,
            )
            .expect("wait before first jitter"),
            ForecastSchedulerDueState::Waiting
        );
        assert_eq!(
            current_forecast_scheduler_due(CAPTURE_END, CAPTURE_END, &NodeWitness, &NodeWitness,)
                .expect("stop at capture end"),
            ForecastSchedulerDueState::Waiting
        );
    }

    #[test]
    fn calculates_exact_node_missed_history_without_startup_backlog() {
        let current = current_forecast_scheduler_due(
            "2026-10-08T18:40:00.000Z",
            CAPTURE_END,
            &NodeWitness,
            &NodeWitness,
        )
        .expect("select catch-up due");
        let ForecastSchedulerDueState::Current(current) = current else {
            panic!("expected current due");
        };
        assert_eq!(
            missed_forecast_scheduler_due_keys(
                Some("2026-10-08T00:40:00.000Z"),
                "2026-10-08T18:40:00.000Z",
                &current.due_key,
                CAPTURE_END,
                &NodeWitness,
                &NodeWitness,
            )
            .expect("calculate missed due keys"),
            [
                "capture/2026-10-08T06:35:00.000Z".to_owned(),
                "capture/2026-10-08T12:35:00.000Z".to_owned(),
            ]
        );
        assert!(
            missed_forecast_scheduler_due_keys(
                None,
                "2026-10-08T18:40:00.000Z",
                &current.due_key,
                CAPTURE_END,
                &NodeWitness,
                &NodeWitness,
            )
            .expect("first startup has no history")
            .is_empty()
        );
    }

    // return one exact four-base witness day
    fn witness_day(midnight: i64) -> Option<ForecastSchedulerUtcDay> {
        let bases = BASE_OFFSETS_MILLISECONDS.map(|offset| {
            let epoch_milliseconds = midnight + offset;
            ForecastSchedulerUtcBase {
                canonical_at: witness_canonical(epoch_milliseconds)
                    .expect("known base witness")
                    .to_owned(),
                epoch_milliseconds,
            }
        });
        Some(ForecastSchedulerUtcDay {
            bases,
            midnight_epoch_milliseconds: midnight,
        })
    }

    // map every base, jitter, and observation used by the Node witness
    fn witness_canonical(epoch_milliseconds: i64) -> Option<&'static str> {
        Some(match epoch_milliseconds {
            1_791_333_300_000 => "2026-10-07T00:35:00.000Z",
            1_791_333_377_000 => "2026-10-07T00:36:17.000Z",
            1_791_354_900_000 => "2026-10-07T06:35:00.000Z",
            1_791_355_122_000 => "2026-10-07T06:38:42.000Z",
            1_791_376_500_000 => "2026-10-07T12:35:00.000Z",
            1_791_376_512_000 => "2026-10-07T12:35:12.000Z",
            1_791_398_100_000 => "2026-10-07T18:35:00.000Z",
            1_791_398_228_000 => "2026-10-07T18:37:08.000Z",
            1_791_419_700_000 => "2026-10-08T00:35:00.000Z",
            1_791_419_891_000 => "2026-10-08T00:38:11.000Z",
            1_791_441_300_000 => "2026-10-08T06:35:00.000Z",
            1_791_441_502_000 => "2026-10-08T06:38:22.000Z",
            1_791_462_900_000 => "2026-10-08T12:35:00.000Z",
            1_791_463_100_000 => "2026-10-08T12:38:20.000Z",
            1_791_484_500_000 => "2026-10-08T18:35:00.000Z",
            1_791_484_592_000 => "2026-10-08T18:36:32.000Z",
            1_791_506_100_000 => "2026-10-09T00:35:00.000Z",
            1_791_506_349_000 => "2026-10-09T00:39:09.000Z",
            1_791_417_600_000 => FORECAST_SCHEDULER_CAPTURE_START,
            1_791_419_760_000 => "2026-10-08T00:36:00.000Z",
            1_791_420_000_000 => "2026-10-08T00:40:00.000Z",
            1_791_441_360_000 => "2026-10-08T06:36:00.000Z",
            1_791_441_600_000 => "2026-10-08T06:40:00.000Z",
            1_791_484_800_000 => "2026-10-08T18:40:00.000Z",
            1_854_601_200_000 => CAPTURE_END,
            _ => return None,
        })
    }

    // invert the exact canonical witness table through the admitted codec seam
    fn witness_milliseconds(text: &str) -> Option<i64> {
        [
            1_791_333_300_000,
            1_791_333_377_000,
            1_791_354_900_000,
            1_791_355_122_000,
            1_791_376_500_000,
            1_791_376_512_000,
            1_791_398_100_000,
            1_791_398_228_000,
            1_791_417_600_000,
            1_791_419_700_000,
            1_791_419_760_000,
            1_791_419_891_000,
            1_791_420_000_000,
            1_791_441_300_000,
            1_791_441_360_000,
            1_791_441_502_000,
            1_791_441_600_000,
            1_791_462_900_000,
            1_791_463_100_000,
            1_791_484_500_000,
            1_791_484_592_000,
            1_791_484_800_000,
            1_791_506_100_000,
            1_791_506_349_000,
            1_854_601_200_000,
        ]
        .into_iter()
        .find(|epoch| witness_canonical(*epoch) == Some(text))
    }
}
