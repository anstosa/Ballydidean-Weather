//! Concrete pinned predecessor calendar for the native isolated wind fitter.

use crate::{
    NativePredecessorModelCalendar, WindFitInputCalendar, WindFitLocalFeatures,
    WindMaintenanceLocalCalendar, format_predecessor_source_utc, parse_predecessor_source_utc,
};
use weather_api::AdmittedWindRowCalendar;
use weather_contracts::ForecastAdjustmentCandidateV2RuntimeFingerprint;
const DAY_MILLISECONDS: i64 = 86_400_000;

/// Real wind fitter calendar deriving the pinned predecessor tzdata and ICU fingerprint.
pub struct NativeWindMaintenanceFitCalendar {
    calendar: NativePredecessorModelCalendar,
}

impl NativeWindMaintenanceFitCalendar {
    /// Require the embedded verified predecessor transition table.
    pub fn new() -> Option<Self> {
        Some(Self {
            calendar: NativePredecessorModelCalendar::new()?,
        })
    }
}

impl WindMaintenanceLocalCalendar for NativeWindMaintenanceFitCalendar {
    /// Resolve exact local midnight using the original wall-clock correction rule.
    fn wind_local_midnight_milliseconds(&self, date: &str, next_day: bool) -> Option<i64> {
        let label = if next_day {
            self.wind_fit_add_local_days(date, 1)?
        } else {
            date.to_owned()
        };
        let target =
            parse_predecessor_source_utc(&format!("{label}T00:00:00.000Z"))?.epoch_milliseconds;
        let mut guess = target.checked_add(8 * 60 * 60 * 1000)?;
        // retain the original bounded correction against pinned local wall-clock features
        for _ in 0..3 {
            let utc = format_predecessor_source_utc(guess)?;
            let offset = i64::from(self.calendar.features(&utc).ok()?.offset_seconds) * 1000;
            guess = guess.checked_add(target.checked_sub(guess.checked_add(offset)?)?)?;
        }
        let features = self
            .calendar
            .features(&format_predecessor_source_utc(guess)?)
            .ok()?;
        (features.local_date == label && features.hour == 0).then_some(guess)
    }
}

impl WindFitInputCalendar for NativeWindMaintenanceFitCalendar {
    /// Admit exact Gregorian day labels as epoch-relative local-day ordinals.
    fn wind_fit_date_ordinal(&self, date: &str) -> Option<i64> {
        let parsed = parse_predecessor_source_utc(&format!("{date}T00:00:00.000Z"))?;
        (parsed.utc.get(..10)? == date)
            .then_some(parsed.epoch_milliseconds.div_euclid(DAY_MILLISECONDS))
    }

    /// Add calendar day labels without applying elapsed UTC time to a local instant.
    fn wind_fit_add_local_days(&self, date: &str, days: u16) -> Option<String> {
        let ordinal = self
            .wind_fit_date_ordinal(date)?
            .checked_add(i64::from(days))?;
        let label = format_predecessor_source_utc(ordinal.checked_mul(DAY_MILLISECONDS)?)?;
        Some(label.get(..10)?.to_owned())
    }

    /// Return the exact source canonical UTC spelling.
    fn wind_fit_utc_at(&self, ms: i64) -> Option<String> {
        format_predecessor_source_utc(ms)
    }

    /// Derive the pinned source local hierarchy features for the admitted instant.
    fn wind_fit_local_features(&self, utc: &str, ms: i64) -> Option<WindFitLocalFeatures> {
        let features = self.calendar.features(utc).ok()?;
        if features.epoch_milliseconds != ms {
            return None;
        }
        Some(WindFitLocalFeatures {
            local_date: features.local_date,
            calendar: AdmittedWindRowCalendar {
                daypart: features.daypart,
                month: u64::from(features.month),
                season: features.season,
            },
        })
    }

    /// Retain the exact fingerprint used by the original fitted artifact.
    fn wind_fit_runtime_fingerprint(&self) -> ForecastAdjustmentCandidateV2RuntimeFingerprint {
        let (_, icu, tzdata) = self.calendar.node_identity();
        ForecastAdjustmentCandidateV2RuntimeFingerprint {
            icuVersion: icu.to_owned(),
            tzdataVersion: tzdata.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Preserve exact Gregorian rollover, DST local midnight and predecessor fingerprint.
    #[test]
    fn native_wind_fit_calendar_matches_source_dst_and_rollover() {
        let calendar = NativeWindMaintenanceFitCalendar::new().unwrap();
        assert_eq!(
            calendar.wind_fit_add_local_days("2024-02-28", 1).as_deref(),
            Some("2024-02-29")
        );
        let spring = calendar
            .wind_local_midnight_milliseconds("2026-03-08", false)
            .unwrap();
        let spring_next = calendar
            .wind_local_midnight_milliseconds("2026-03-08", true)
            .unwrap();
        assert_eq!(spring_next - spring, 23 * 60 * 60 * 1000);
        let autumn = calendar
            .wind_local_midnight_milliseconds("2026-11-01", false)
            .unwrap();
        let autumn_next = calendar
            .wind_local_midnight_milliseconds("2026-11-01", true)
            .unwrap();
        assert_eq!(autumn_next - autumn, 25 * 60 * 60 * 1000);
        assert_eq!(calendar.wind_fit_runtime_fingerprint().icuVersion, "78.3");
        assert_eq!(
            calendar.wind_fit_runtime_fingerprint().tzdataVersion,
            "2026b"
        );
    }
}
