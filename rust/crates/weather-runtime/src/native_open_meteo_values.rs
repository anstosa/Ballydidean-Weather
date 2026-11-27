//! Native Open-Meteo number, receipt-clock, and provider-calendar primitives.

use std::path::{Path, PathBuf};

use jiff::{Timestamp, civil::DateTime, tz::TimeZone};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;
use weather_worker::{
    ForecastAnchorBackfillOperationError, OpenMeteoArchiveInstant, TideBackfillErrorClassification,
    TideBackfillOperationError,
};

use crate::{
    NativeProcessClock, NativeWirePrimitives, OpenMeteoArchiveValues, OpenMeteoPreviousRunsValues,
    native_json::encode_ordered_json, system_worker_health_milliseconds,
};

const QUARTER_HOUR_SECONDS: i32 = 15 * 60;
const MAXIMUM_PROVIDER_OFFSET_SECONDS: i32 = 14 * 60 * 60;

/// Injectable receipt clock used by both native Open-Meteo provider owners.
pub trait NativeOpenMeteoClock {
    /// Return one current Unix epoch sample at millisecond precision.
    fn now_epoch_milliseconds(&mut self) -> Option<i64>;
}

impl NativeOpenMeteoClock for NativeProcessClock {
    /// Sample the same real process clock used by the other native owners.
    fn now_epoch_milliseconds(&mut self) -> Option<i64> {
        system_worker_health_milliseconds()
    }
}

/// Native values owner shared by Archive and Previous Runs operations.
#[derive(Debug)]
pub struct NativeOpenMeteoValues<Clock = NativeProcessClock> {
    clock: Clock,
    timezone: Option<CachedTimeZone>,
}

#[derive(Debug)]
struct CachedTimeZone {
    name: String,
    value: TimeZone,
}

impl NativeOpenMeteoValues<NativeProcessClock> {
    /// Construct the production owner with the actual process clock.
    pub const fn new() -> Self {
        Self::with_clock(NativeProcessClock)
    }
}

impl<Clock> NativeOpenMeteoValues<Clock> {
    /// Construct an owner with an injected receipt clock.
    pub const fn with_clock(clock: Clock) -> Self {
        Self {
            clock,
            timezone: None,
        }
    }

    /// Load a named zone once and retain it for every row in the response.
    fn timezone(&mut self, name: &str) -> Option<&TimeZone> {
        // reload only when a later operation selects a different provider zone
        if self
            .timezone
            .as_ref()
            .is_none_or(|cached| cached.name != name)
        {
            self.timezone = Some(CachedTimeZone {
                name: name.to_owned(),
                value: load_named_timezone(name)?,
            });
        }
        self.timezone.as_ref().map(|cached| &cached.value)
    }
}

impl Default for NativeOpenMeteoValues<NativeProcessClock> {
    /// Construct the production values owner.
    fn default() -> Self {
        Self::new()
    }
}

impl<Clock: NativeOpenMeteoClock> NativeOpenMeteoValues<Clock> {
    /// Format one fallible clock sample through the canonical native UTC primitive.
    fn canonical_receipt(&mut self) -> Result<String, TideBackfillOperationError> {
        let milliseconds = self
            .clock
            .now_epoch_milliseconds()
            .ok_or_else(|| unexpected("Open-Meteo receipt clock is unavailable"))?;
        NativeWirePrimitives
            .format_utc_milliseconds(milliseconds)
            .ok_or_else(|| unexpected("Open-Meteo receipt clock is outside the supported range"))
    }
}

impl<Clock: NativeOpenMeteoClock> OpenMeteoArchiveValues for NativeOpenMeteoValues<Clock> {
    /// Validate the zone by parsing its host TZif rules with Jiff.
    fn validate_timezone(&mut self, timezone: &str) -> Result<(), TideBackfillOperationError> {
        self.timezone(timezone)
            .map(|_| ())
            .ok_or_else(|| invalid("provider timezone is invalid"))
    }

    /// Resolve explicit instants or select the next exact IANA wall-time candidate.
    fn resolve_provider_time(
        &mut self,
        wall_time: &str,
        timezone: &str,
        fallback_offset_seconds: i32,
        previous_valid_at: Option<&OpenMeteoArchiveInstant>,
    ) -> Result<OpenMeteoArchiveInstant, TideBackfillOperationError> {
        // preserve the original direct path for timestamps carrying an explicit offset
        if has_explicit_offset(wall_time) {
            // reject offsets outside the ECMAScript ISO grammar before Jiff parsing
            if !explicit_offset_is_valid(wall_time) {
                return Err(invalid("provider time is invalid"));
            }
            let milliseconds = NativeWirePrimitives
                .parse_explicit_utc_milliseconds(wall_time)
                .ok_or_else(|| invalid("provider time is invalid"))?;
            let utc = NativeWirePrimitives
                .format_utc_milliseconds(milliseconds)
                .ok_or_else(|| invalid("provider time is invalid"))?;
            return Ok(OpenMeteoArchiveInstant {
                epoch_milliseconds: milliseconds,
                utc,
            });
        }

        let local = parse_provider_wall_time(wall_time)?;
        let zone = self
            .timezone(timezone)
            .ok_or_else(|| invalid("provider timezone is invalid"))?
            .clone();
        let fallback_minutes = javascript_round_minutes(fallback_offset_seconds);
        let mut candidates = Vec::with_capacity(2);

        // inspect both ambiguity choices and retain only exact wall-time mappings
        for candidate in [
            zone.to_ambiguous_timestamp(local).earlier(),
            zone.to_ambiguous_timestamp(local).later(),
        ] {
            let Ok(candidate) = candidate else {
                continue;
            };
            // gaps map to a neighboring civil time and must not be normalized silently
            if zone.to_datetime(candidate) != local {
                continue;
            }
            let offset_seconds = zone.to_offset(candidate).seconds();
            // match the predecessor's modern quarter-hour search plus its rounded fallback
            if !admitted_provider_offset(offset_seconds, fallback_minutes) {
                continue;
            }
            candidates.push(candidate.as_millisecond());
        }
        candidates.sort_unstable();
        candidates.dedup();
        let previous = previous_valid_at.map(|instant| instant.epoch_milliseconds);
        let milliseconds = candidates
            .into_iter()
            .find(|candidate| previous.is_none_or(|previous| *candidate > previous))
            .ok_or_else(|| invalid("provider time does not map monotonically in its timezone"))?;
        let utc = NativeWirePrimitives
            .format_utc_milliseconds(milliseconds)
            .ok_or_else(|| invalid("provider time is invalid"))?;
        Ok(OpenMeteoArchiveInstant {
            epoch_milliseconds: milliseconds,
            utc,
        })
    }

    /// Capture one receipt instant after the successful Archive response.
    fn received_at(&mut self) -> Result<String, TideBackfillOperationError> {
        self.canonical_receipt()
    }

    /// Render the exact finite JavaScript number spelling used in metadata.
    fn number_text(&mut self, value: f64) -> String {
        javascript_number_text(value)
    }
}

impl<Clock: NativeOpenMeteoClock> OpenMeteoPreviousRunsValues for NativeOpenMeteoValues<Clock> {
    /// Render the exact finite JavaScript number spelling used in coordinate lists.
    fn number_text(&mut self, value: f64) -> String {
        javascript_number_text(value)
    }

    /// Parse the predecessor's two accepted zero-offset hourly wall-time spellings.
    fn utc_hour(
        &mut self,
        value: &str,
    ) -> Result<(String, i64), ForecastAnchorBackfillOperationError> {
        let local = parse_previous_runs_hour(value)?;
        let canonical = format!(
            "{:04}-{:02}-{:02}T{:02}:00:00.000Z",
            local.year(),
            local.month(),
            local.day(),
            local.hour()
        );
        let timestamp = canonical
            .parse::<Timestamp>()
            .map_err(|_| invalid("Previous Runs time is invalid"))?;
        Ok((canonical, timestamp.as_millisecond()))
    }

    /// Capture one receipt instant after the successful Previous Runs response.
    fn received_at(&mut self) -> Result<String, ForecastAnchorBackfillOperationError> {
        self.canonical_receipt()
    }
}

/// Load one case-insensitive IANA identity from the host zoneinfo database.
pub(crate) fn load_named_timezone(name: &str) -> Option<TimeZone> {
    let path = resolve_zoneinfo_path(name)?;
    let bytes = std::fs::read(path).ok()?;
    TimeZone::tzif(name, &bytes).ok()
}

/// Resolve safe path components without changing Intl's case-insensitive lookup behavior.
fn resolve_zoneinfo_path(name: &str) -> Option<PathBuf> {
    let root = Path::new("/usr/share/zoneinfo").canonicalize().ok()?;
    let mut current = root.clone();

    // resolve each component against the actual directory spelling
    for component in name.split('/') {
        // reject empty or traversal components before touching the filesystem
        if component.is_empty() || matches!(component, "." | "..") {
            return None;
        }
        let matched = std::fs::read_dir(&current).ok()?.flatten().find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(component))
        })?;
        current.push(matched.file_name());
    }

    let resolved = current.canonicalize().ok()?;
    // keep aliases inside the host database and reject directories as zones
    if !resolved.starts_with(root) || !resolved.is_file() {
        return None;
    }
    Some(resolved)
}

/// Recognize the explicit suffix accepted by the original provider conversion.
fn has_explicit_offset(value: &str) -> bool {
    let bytes = value.as_bytes();
    // retain uppercase Z exactly as the source regular expression does
    if bytes.last() == Some(&b'Z') {
        return true;
    }
    matches!(
        bytes.get(bytes.len().saturating_sub(6)..),
        Some([b'+' | b'-', hour_tens, hour_ones, b':', minute_tens, minute_ones])
            if [hour_tens, hour_ones, minute_tens, minute_ones]
                .iter()
                .all(|digit| digit.is_ascii_digit())
    )
}

/// Enforce the source Date parser's maximum signed ISO offset.
fn explicit_offset_is_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    // uppercase Z carries no numeric offset to validate
    if bytes.last() == Some(&b'Z') {
        return true;
    }
    let Some(offset) = bytes.get(bytes.len().saturating_sub(6)..) else {
        return false;
    };
    let hours = decimal_i8(&offset[1..3]);
    let minutes = decimal_i8(&offset[4..6]);
    matches!((hours, minutes), (Some(hours), Some(minutes)) if hours <= 23 && minutes <= 59)
}

/// Parse one exact local minute or non-zero-second provider wall time.
fn parse_provider_wall_time(value: &str) -> Result<DateTime, TideBackfillOperationError> {
    let bytes = value.as_bytes();
    let shape_matches = matches!(bytes.len(), 16 | 19)
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b'T')
        && bytes.get(13) == Some(&b':')
        && (bytes.len() == 16 || bytes.get(16) == Some(&b':'))
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7 | 10 | 13 | 16) || byte.is_ascii_digit());
    // reject text outside the documented minute-resolution shape
    if !shape_matches {
        return Err(invalid("provider time must use local ISO minute precision"));
    }
    // retain non-zero seconds only when the provider supplied the optional component
    let second = if bytes.len() == 19 {
        decimal_i8(&bytes[17..19]).ok_or_else(|| invalid("provider time is invalid"))?
    } else {
        0
    };
    let local = DateTime::new(
        decimal_i16(&bytes[0..4]).ok_or_else(|| invalid("provider time is invalid"))?,
        decimal_i8(&bytes[5..7]).ok_or_else(|| invalid("provider time is invalid"))?,
        decimal_i8(&bytes[8..10]).ok_or_else(|| invalid("provider time is invalid"))?,
        decimal_i8(&bytes[11..13]).ok_or_else(|| invalid("provider time is invalid"))?,
        decimal_i8(&bytes[14..16]).ok_or_else(|| invalid("provider time is invalid"))?,
        second,
        0,
    )
    .map_err(|_| invalid("provider time is invalid"))?;
    // reproduce the Intl formatter's omission of a zero-second component
    let canonical = if second == 0 {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}",
            local.year(),
            local.month(),
            local.day(),
            local.hour(),
            local.minute()
        )
    } else {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            local.year(),
            local.month(),
            local.day(),
            local.hour(),
            local.minute(),
            local.second()
        )
    };
    // mirror Intl formatting, which omits an explicit zero-second component
    if canonical != value {
        return Err(invalid(
            "provider time does not map monotonically in its timezone",
        ));
    }
    Ok(local)
}

/// Parse one Previous Runs UTC hour without permitting calendar rollover.
fn parse_previous_runs_hour(value: &str) -> Result<DateTime, ForecastAnchorBackfillOperationError> {
    let bytes = value.as_bytes();
    let shape_matches = matches!(bytes.len(), 16 | 19)
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b'T')
        && bytes.get(13..16) == Some(b":00")
        && (bytes.len() == 16 || bytes.get(16..19) == Some(b":00"))
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7 | 10 | 13 | 16) || byte.is_ascii_digit());
    // retain the original precision diagnostic before validating the calendar
    if !shape_matches {
        return Err(invalid(
            "Previous Runs time must use UTC hourly ISO precision",
        ));
    }
    DateTime::new(
        decimal_i16(&bytes[0..4]).ok_or_else(|| invalid("Previous Runs time is invalid"))?,
        decimal_i8(&bytes[5..7]).ok_or_else(|| invalid("Previous Runs time is invalid"))?,
        decimal_i8(&bytes[8..10]).ok_or_else(|| invalid("Previous Runs time is invalid"))?,
        decimal_i8(&bytes[11..13]).ok_or_else(|| invalid("Previous Runs time is invalid"))?,
        0,
        0,
        0,
    )
    .map_err(|_| invalid("Previous Runs time is invalid"))
}

/// Parse one fixed-width decimal component into Jiff's year representation.
fn decimal_i16(bytes: &[u8]) -> Option<i16> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Parse one fixed-width decimal component into Jiff's calendar representation.
fn decimal_i8(bytes: &[u8]) -> Option<i8> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Reproduce JavaScript Math.round for an integer count divided by sixty.
fn javascript_round_minutes(seconds: i32) -> i32 {
    let minutes = seconds.div_euclid(60);
    let remainder = seconds.rem_euclid(60);
    // ties and larger fractions round toward positive infinity
    if remainder >= 30 {
        minutes + 1
    } else {
        minutes
    }
}

/// Retain only offsets present in the predecessor's bounded candidate search.
fn admitted_provider_offset(offset_seconds: i32, fallback_minutes: i32) -> bool {
    offset_seconds == fallback_minutes * 60
        || (-MAXIMUM_PROVIDER_OFFSET_SECONDS..=MAXIMUM_PROVIDER_OFFSET_SECONDS)
            .contains(&offset_seconds)
            && offset_seconds % QUARTER_HOUR_SECONDS == 0
}

/// render the exact javascript number spelling
pub(crate) fn javascript_number_text(value: f64) -> String {
    let encoded = encode_ordered_json(&JsonValue::Number(value))
        .expect("finite Open-Meteo coordinates must be JSON encodable");
    let bytes = encoded
        .strip_suffix(b"\n")
        .expect("native JSON must end with one line feed");
    std::str::from_utf8(bytes)
        .expect("JavaScript number spellings must be ASCII")
        .to_owned()
}

/// Construct one classified provider payload failure.
fn invalid(message: impl Into<String>) -> TideBackfillOperationError {
    TideBackfillOperationError::provider(
        1,
        TideBackfillErrorClassification::InvalidPayload,
        "invalid_payload",
        message,
    )
}

/// Construct one failure outside the provider payload classification boundary.
fn unexpected(message: impl Into<String>) -> TideBackfillOperationError {
    TideBackfillOperationError::unexpected(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FixedClock(Option<i64>);

    impl NativeOpenMeteoClock for FixedClock {
        /// Return the injected sample without consulting the process clock.
        fn now_epoch_milliseconds(&mut self) -> Option<i64> {
            self.0
        }
    }

    /// Match the predecessor's explicit-offset, fold, gap, and monotonic policies.
    #[test]
    fn archive_times_match_provider_calendar_selection() {
        let mut values = NativeOpenMeteoValues::with_clock(FixedClock(Some(0)));
        values.validate_timezone("america/los_angeles").unwrap();
        let explicit = values
            .resolve_provider_time("2026-01-02T03:04:05+02:30", "UTC", 0, None)
            .unwrap();
        assert_eq!(explicit.utc, "2026-01-02T00:34:05.000Z");

        let first = values
            .resolve_provider_time("2026-11-01T01:00", "America/Los_Angeles", 0, None)
            .unwrap();
        let second = values
            .resolve_provider_time("2026-11-01T01:00", "America/Los_Angeles", 0, Some(&first))
            .unwrap();
        assert_eq!(first.utc, "2026-11-01T08:00:00.000Z");
        assert_eq!(second.utc, "2026-11-01T09:00:00.000Z");
        assert!(
            values
                .resolve_provider_time("2026-03-08T02:00", "America/Los_Angeles", -28_800, None)
                .is_err()
        );
    }

    /// Match Previous Runs hourly parsing, receipt formatting, and JavaScript numbers.
    #[test]
    fn previous_runs_values_match_original_wire_spelling() {
        let mut values = NativeOpenMeteoValues::with_clock(FixedClock(Some(0)));
        assert_eq!(
            OpenMeteoPreviousRunsValues::utc_hour(&mut values, "2024-02-29T23:00").unwrap(),
            ("2024-02-29T23:00:00.000Z".to_owned(), 1_709_247_600_000)
        );
        assert_eq!(
            OpenMeteoPreviousRunsValues::utc_hour(&mut values, "2024-02-29T23:00:00")
                .unwrap()
                .0,
            "2024-02-29T23:00:00.000Z"
        );
        assert!(OpenMeteoPreviousRunsValues::utc_hour(&mut values, "2023-02-29T23:00").is_err());
        assert_eq!(
            OpenMeteoPreviousRunsValues::received_at(&mut values).unwrap(),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            OpenMeteoPreviousRunsValues::number_text(&mut values, -0.0),
            "0"
        );
        assert_eq!(
            OpenMeteoPreviousRunsValues::number_text(&mut values, 1e-7),
            "1e-7"
        );
        assert_eq!(
            OpenMeteoPreviousRunsValues::number_text(
                &mut values,
                f64::from_bits(0x3ff0_0008_0000_0000)
            ),
            "1.0000076293945312"
        );
        assert_eq!(
            OpenMeteoPreviousRunsValues::number_text(&mut values, 1e21),
            "1e+21"
        );
    }
}
