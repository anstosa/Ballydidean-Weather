//! Embedded predecessor calendar authority generated from pinned Node Temporal and Intl.

use std::{fmt::Write as _, sync::OnceLock};

use weather_api::{
    AdmittedWindRowCalendar, CanonicalInstant, CurrentProjectionError, MaintenanceWindModelCalendar,
};
use weather_contracts::{
    ForecastAdjustmentCoefficientDaypartEnum, ForecastAdjustmentCoefficientSeasonEnum,
};
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::NativeWirePrimitives;

const TIMEZONE: &str = "America/Los_Angeles";
const NODE_VERSION: &str = "24.16.0";
const ICU_VERSION: &str = "78.3";
const TZDATA_VERSION: &str = "2026b";
const SOURCE_START_MILLISECONDS: i64 = -62_167_219_200_000;
const SOURCE_END_MILLISECONDS: i64 = 253_402_300_799_999;
const TRANSITION_END_MILLISECONDS: i64 = SOURCE_END_MILLISECONDS;
const INITIAL_OFFSET_SECONDS: i32 = -28_378;
const TRANSITION_COUNT: usize = 16_109;
const HEADER_BYTES: usize = 32;
const ENTRY_BYTES: usize = 12;
const DAY_MILLISECONDS: i64 = 86_400_000;
const ARTIFACT_SHA256: &str = "f93a1d92f8221a047aaccd5796b1da3fee9ab30f5eb482bb8df8431fe65e8a00";
const ARTIFACT: &[u8] = include_bytes!("../native-calendar/america-los-angeles-node-24.16.0.bin");

static ARTIFACT_VALID: OnceLock<bool> = OnceLock::new();

/// Exact predecessor local fields used by the wind and temperature model calendars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativePredecessorCalendarFeatures {
    pub daypart: ForecastAdjustmentCoefficientDaypartEnum,
    pub epoch_milliseconds: i64,
    pub hour: u8,
    pub local_astronomical_year: i64,
    pub local_day: u8,
    pub local_date: String,
    pub month: u8,
    pub offset_seconds: i32,
    pub season: ForecastAdjustmentCoefficientSeasonEnum,
    pub timezone: &'static str,
    pub valid_at: String,
}

/// Immutable native authority over the exact generated predecessor transition table.
#[derive(Clone, Copy, Debug)]
pub struct NativePredecessorModelCalendar {
    _verified: (),
}

impl NativePredecessorModelCalendar {
    /// Admit the embedded table only after its shape, order, bounds, and digest are verified.
    pub fn new() -> Option<Self> {
        (*ARTIFACT_VALID.get_or_init(artifact_is_valid)).then_some(Self { _verified: () })
    }

    /// Resolve one complete original four-digit UTC instant without depending on Jiff's range.
    pub fn features(
        &self,
        valid_at: &str,
    ) -> Result<NativePredecessorCalendarFeatures, CurrentProjectionError> {
        let parsed = parse_predecessor_source_utc(valid_at).ok_or_else(calendar_error)?;
        let offset_seconds =
            offset_seconds_at(parsed.epoch_milliseconds).ok_or_else(calendar_error)?;
        let local_milliseconds = parsed
            .epoch_milliseconds
            .checked_add(
                i64::from(offset_seconds)
                    .checked_mul(1_000)
                    .ok_or_else(calendar_error)?,
            )
            .ok_or_else(calendar_error)?;
        let local_days = local_milliseconds.div_euclid(DAY_MILLISECONDS);
        let time_milliseconds = local_milliseconds.rem_euclid(DAY_MILLISECONDS);
        let (astronomical_year, month, day) =
            civil_from_days(local_days).ok_or_else(calendar_error)?;
        // mirror Intl's era display while retaining astronomical year zero internally
        let display_year = if astronomical_year <= 0 {
            1i64.checked_sub(astronomical_year)
                .ok_or_else(calendar_error)?
        } else {
            astronomical_year
        };
        let hour = u8::try_from(time_milliseconds / 3_600_000).map_err(|_| calendar_error())?;
        let month = u8::try_from(month).map_err(|_| calendar_error())?;
        let local_day = u8::try_from(day).map_err(|_| calendar_error())?;
        Ok(NativePredecessorCalendarFeatures {
            daypart: daypart(hour)?,
            epoch_milliseconds: parsed.epoch_milliseconds,
            hour,
            local_astronomical_year: astronomical_year,
            local_day,
            local_date: format!("{display_year}-{month:02}-{day:02}"),
            month,
            offset_seconds,
            season: season(month)?,
            timezone: TIMEZONE,
            valid_at: parsed.utc,
        })
    }

    /// Admit only the exact Node runtime fingerprint that generated the embedded authority.
    pub fn fingerprint_matches(&self, icu_version: &str, tzdata_version: &str) -> bool {
        icu_version == ICU_VERSION && tzdata_version == TZDATA_VERSION
    }

    /// Return the pinned predecessor runtime identity for startup evidence.
    pub const fn node_identity(&self) -> (&'static str, &'static str, &'static str) {
        (NODE_VERSION, ICU_VERSION, TZDATA_VERSION)
    }
}

impl MaintenanceWindModelCalendar for NativePredecessorModelCalendar {
    /// Derive wind hierarchy fields from the exact predecessor transition table.
    fn calendar(&self, valid_at: &str) -> Result<AdmittedWindRowCalendar, CurrentProjectionError> {
        let features = self.features(valid_at)?;
        Ok(AdmittedWindRowCalendar {
            daypart: features.daypart,
            month: u64::from(features.month),
            season: features.season,
        })
    }

    /// Bind model admission to the generated Node fingerprint and validated table bytes.
    fn fingerprint_matches(&self, icu_version: &str, tzdata_version: &str) -> bool {
        NativePredecessorModelCalendar::fingerprint_matches(self, icu_version, tzdata_version)
    }
}

/// Parse the model-admitted four-digit UTC grammar into a canonical predecessor instant.
pub fn parse_predecessor_source_utc(value: &str) -> Option<CanonicalInstant> {
    let bytes = value.as_bytes();
    // retain only the source's optional-millisecond UTC forms
    if !matches!(bytes.len(), 20 | 24)
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || bytes.last() != Some(&b'Z')
        || bytes.len() == 24 && bytes.get(19) != Some(&b'.')
    {
        return None;
    }
    let year = digits(bytes, 0, 4)?;
    let month = digits(bytes, 5, 2)?;
    let day = digits(bytes, 8, 2)?;
    let hour = digits(bytes, 11, 2)?;
    let minute = digits(bytes, 14, 2)?;
    let second = digits(bytes, 17, 2)?;
    let millisecond = if bytes.len() == 24 {
        digits(bytes, 20, 3)?
    } else {
        0
    };
    // reject Date-normalized fields before deriving the epoch count
    if !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)?).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day))?;
    let day_milliseconds = i64::from(hour)
        .checked_mul(3_600_000)?
        .checked_add(i64::from(minute).checked_mul(60_000)?)?
        .checked_add(i64::from(second).checked_mul(1_000)?)?
        .checked_add(i64::from(millisecond))?;
    let milliseconds = days
        .checked_mul(DAY_MILLISECONDS)?
        .checked_add(day_milliseconds)?;
    // bind the arithmetic parser to the exact original source domain
    if !(SOURCE_START_MILLISECONDS..=SOURCE_END_MILLISECONDS).contains(&milliseconds) {
        return None;
    }
    Some(CanonicalInstant {
        epoch_milliseconds: milliseconds,
        utc: format!(
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z"
        ),
    })
}

/// Format one admitted source epoch as its exact four-digit canonical UTC spelling.
pub fn format_predecessor_source_utc(epoch_milliseconds: i64) -> Option<String> {
    // retain only the original model's complete four-digit source domain
    if !(SOURCE_START_MILLISECONDS..=SOURCE_END_MILLISECONDS).contains(&epoch_milliseconds) {
        return None;
    }
    let days = epoch_milliseconds.div_euclid(DAY_MILLISECONDS);
    let time_milliseconds = epoch_milliseconds.rem_euclid(DAY_MILLISECONDS);
    let (year, month, day) = civil_from_days(days)?;
    let hour = time_milliseconds / 3_600_000;
    let minute = time_milliseconds.rem_euclid(3_600_000) / 60_000;
    let second = time_milliseconds.rem_euclid(60_000) / 1_000;
    let millisecond = time_milliseconds.rem_euclid(1_000);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z"
    ))
}

/// Parse one fixed-width ASCII decimal field without Unicode or sign aliases.
fn digits(bytes: &[u8], start: usize, length: usize) -> Option<u16> {
    let end = start.checked_add(length)?;
    let mut value = 0u16;
    // consume every required digit exactly once
    for byte in bytes.get(start..end)? {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value
            .checked_mul(10)?
            .checked_add(u16::from(*byte - b'0'))?;
    }
    Some(value)
}

/// Return the proleptic Gregorian month length including astronomical year zero.
fn days_in_month(year: u16, month: u16) -> Option<u16> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(i64::from(year)) => 29,
        2 => 28,
        _ => return None,
    })
}

/// Convert one Gregorian civil date to Unix-relative days using astronomical years.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    // shift January and February into the preceding algorithmic year
    let january_or_february = if month <= 2 { 1 } else { 0 };
    let adjusted_year = year.checked_sub(january_or_february)?;
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year.checked_sub(era.checked_mul(400)?)?;
    let shifted_month = month.checked_add(if month > 2 { -3 } else { 9 })?;
    let day_of_year = shifted_month
        .checked_mul(153)?
        .checked_add(2)?
        .div_euclid(5)
        .checked_add(day)?
        .checked_sub(1)?;
    let day_of_era = year_of_era
        .checked_mul(365)?
        .checked_add(year_of_era / 4)?
        .checked_sub(year_of_era / 100)?
        .checked_add(day_of_year)?;
    era.checked_mul(146_097)?
        .checked_add(day_of_era)?
        .checked_sub(719_468)
}

/// Convert Unix-relative days back to exact proleptic Gregorian civil fields.
fn civil_from_days(days: i64) -> Option<(i64, i64, i64)> {
    let shifted = days.checked_add(719_468)?;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.checked_sub(era.checked_mul(146_097)?)?;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era.checked_add(era.checked_mul(400)?)?;
    let day_of_year = day_of_era.checked_sub(
        year_of_era
            .checked_mul(365)
            .and_then(|value| value.checked_add(year_of_era / 4))
            .and_then(|value| value.checked_sub(year_of_era / 100))?,
    )?;
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = shifted_month + if shifted_month < 10 { 3 } else { -9 };
    // restore the civil year after converting the shifted March-based month
    year = year.checked_add(if month <= 2 { 1 } else { 0 })?;
    Some((year, month, day))
}

/// Select the generated predecessor offset at one admitted source instant.
fn offset_seconds_at(milliseconds: i64) -> Option<i32> {
    if !(SOURCE_START_MILLISECONDS..=SOURCE_END_MILLISECONDS).contains(&milliseconds) {
        return None;
    }
    let count = artifact_count()?;
    let mut low = 0usize;
    let mut high = count;
    // find the first transition strictly after the requested instant
    while low < high {
        let middle = low + (high - low) / 2;
        if transition_milliseconds(middle)? <= milliseconds {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    if low == 0 {
        Some(INITIAL_OFFSET_SECONDS)
    } else {
        transition_offset_seconds(low - 1)
    }
}

/// Verify the generated table before any fingerprint or feature admission.
fn artifact_is_valid() -> bool {
    if ARTIFACT.len() != HEADER_BYTES + TRANSITION_COUNT * ENTRY_BYTES
        || ARTIFACT.get(0..8) != Some(&b"NCALV1\0\0"[..])
        || read_i64(8) != Some(SOURCE_START_MILLISECONDS)
        || read_i64(16) != Some(TRANSITION_END_MILLISECONDS)
        || read_i32(24) != Some(INITIAL_OFFSET_SECONDS)
        || artifact_count() != Some(TRANSITION_COUNT)
        || artifact_sha256() != ARTIFACT_SHA256
    {
        return false;
    }
    let mut previous = SOURCE_START_MILLISECONDS;
    // prove strict ordering and bounded integral offsets for every generated entry
    for index in 0..TRANSITION_COUNT {
        let Some(milliseconds) = transition_milliseconds(index) else {
            return false;
        };
        let Some(offset) = transition_offset_seconds(index) else {
            return false;
        };
        if milliseconds <= previous
            || milliseconds > TRANSITION_END_MILLISECONDS
            || !(-86_400..=86_400).contains(&offset)
        {
            return false;
        }
        previous = milliseconds;
    }
    true
}

/// Read the generated transition count from its fixed header.
fn artifact_count() -> Option<usize> {
    usize::try_from(read_u32(28)?).ok()
}

/// Read one transition timestamp from its fixed-width generated entry.
fn transition_milliseconds(index: usize) -> Option<i64> {
    let offset = HEADER_BYTES.checked_add(index.checked_mul(ENTRY_BYTES)?)?;
    read_i64(offset)
}

/// Read one post-transition offset from its fixed-width generated entry.
fn transition_offset_seconds(index: usize) -> Option<i32> {
    let offset = HEADER_BYTES
        .checked_add(index.checked_mul(ENTRY_BYTES)?)?
        .checked_add(8)?;
    read_i32(offset)
}

/// Decode one little-endian signed 64-bit artifact field.
fn read_i64(offset: usize) -> Option<i64> {
    Some(i64::from_le_bytes(
        ARTIFACT
            .get(offset..offset.checked_add(8)?)?
            .try_into()
            .ok()?,
    ))
}

/// Decode one little-endian signed 32-bit artifact field.
fn read_i32(offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        ARTIFACT
            .get(offset..offset.checked_add(4)?)?
            .try_into()
            .ok()?,
    ))
}

/// Decode one little-endian unsigned 32-bit artifact field.
fn read_u32(offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        ARTIFACT
            .get(offset..offset.checked_add(4)?)?
            .try_into()
            .ok()?,
    ))
}

/// Hash the immutable table bytes using the approved native SHA owner.
fn artifact_sha256() -> String {
    let digest = RevisionArchiveCanonicalCodec::sha256(&NativeWirePrimitives::new(), ARTIFACT);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

/// Map one predecessor local hour into its exact six-hour hierarchy band.
fn daypart(hour: u8) -> Result<ForecastAdjustmentCoefficientDaypartEnum, CurrentProjectionError> {
    match hour {
        0..=5 => Ok(ForecastAdjustmentCoefficientDaypartEnum::Night),
        6..=11 => Ok(ForecastAdjustmentCoefficientDaypartEnum::Morning),
        12..=17 => Ok(ForecastAdjustmentCoefficientDaypartEnum::Afternoon),
        18..=23 => Ok(ForecastAdjustmentCoefficientDaypartEnum::Evening),
        _ => Err(calendar_error()),
    }
}

/// Map one predecessor local month into its northern meteorological season.
fn season(month: u8) -> Result<ForecastAdjustmentCoefficientSeasonEnum, CurrentProjectionError> {
    match month {
        12 | 1 | 2 => Ok(ForecastAdjustmentCoefficientSeasonEnum::Winter),
        3..=5 => Ok(ForecastAdjustmentCoefficientSeasonEnum::Spring),
        6..=8 => Ok(ForecastAdjustmentCoefficientSeasonEnum::Summer),
        9..=11 => Ok(ForecastAdjustmentCoefficientSeasonEnum::Autumn),
        _ => Err(calendar_error()),
    }
}

/// Apply the proleptic Gregorian leap-year rule including astronomical year zero.
const fn is_leap_year(year: i64) -> bool {
    year % 400 == 0 || (year % 4 == 0 && year % 100 != 0)
}

/// Construct one bounded model-calendar failure without exposing artifact contents.
fn calendar_error() -> CurrentProjectionError {
    CurrentProjectionError {
        code: "api.model_calendar_invalid",
        detail: "predecessor model calendar input or generated authority is invalid".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preserve pinned fingerprint admission and every reviewed Intl year-edge spelling.
    #[test]
    fn generated_predecessor_calendar_covers_complete_source_domain() {
        let calendar = NativePredecessorModelCalendar::new().unwrap();
        assert!(calendar.fingerprint_matches(ICU_VERSION, TZDATA_VERSION));
        assert!(!calendar.fingerprint_matches(ICU_VERSION, "2026f"));

        let expectations = [
            ("0000-01-01T00:00:00.000Z", "2-12-31", 16, -28_378),
            ("0001-01-01T00:00:00.000Z", "1-12-31", 16, -28_378),
            ("0099-01-01T00:00:00.000Z", "98-12-31", 16, -28_378),
            ("0100-01-01T00:00:00.000Z", "99-12-31", 16, -28_378),
            ("0999-01-01T00:00:00.000Z", "998-12-31", 16, -28_378),
            ("1000-01-01T00:00:00.000Z", "999-12-31", 16, -28_378),
            ("2026-03-08T09:59:59.999Z", "2026-03-08", 1, -28_800),
            ("2026-03-08T10:00:00.000Z", "2026-03-08", 3, -25_200),
            ("9999-12-31T23:59:59.999Z", "9999-12-31", 15, -28_800),
        ];
        // compare the exact source labels and offsets emitted by pinned Node Intl
        for (instant, local_date, hour, offset) in expectations {
            let features = calendar.features(instant).unwrap();
            assert_eq!(features.local_date, local_date);
            assert_eq!(features.hour, hour);
            assert_eq!(features.offset_seconds, offset);
        }
        let first = calendar.features("0000-01-01T00:00:00.000Z").unwrap();
        assert_eq!(first.epoch_milliseconds, SOURCE_START_MILLISECONDS);
        assert_eq!(first.local_astronomical_year, -1);
        assert_eq!(first.local_day, 31);
    }

    /// Reject normalized aliases while accepting the source's optional millisecond form.
    #[test]
    fn predecessor_calendar_keeps_canonical_utc_grammar() {
        let calendar = NativePredecessorModelCalendar::new().unwrap();
        assert_eq!(
            calendar.features("2026-09-12T21:00:00Z").unwrap().valid_at,
            "2026-09-12T21:00:00.000Z"
        );
        assert!(calendar.features("2026-02-29T00:00:00.000Z").is_err());
        assert!(calendar.features("+002026-09-12T21:00:00.000Z").is_err());
        let edge_instants = [
            SOURCE_START_MILLISECONDS,
            -1,
            0,
            1_778_565_600_000,
            SOURCE_END_MILLISECONDS,
        ];
        // prove the pure formatter and parser retain every checked source millisecond
        for epoch_milliseconds in edge_instants {
            let utc = format_predecessor_source_utc(epoch_milliseconds).unwrap();
            let parsed = parse_predecessor_source_utc(&utc).unwrap();
            assert_eq!(parsed.epoch_milliseconds, epoch_milliseconds);
            assert_eq!(parsed.utc, utc);
        }
        assert!(format_predecessor_source_utc(SOURCE_START_MILLISECONDS - 1).is_none());
        assert!(format_predecessor_source_utc(SOURCE_END_MILLISECONDS + 1).is_none());
    }
}
