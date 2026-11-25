//! Native civil-time owner for retained historical backfill paths.

use jiff::{Span, Timestamp, civil::Date};
use weather_edge::ArchiveWirePrimitives;
use weather_providers::ProviderControls;
use weather_worker::{
    HistoricalBackfillCommandDate, HistoricalBackfillCommandTimeAdapter, OpenMeteoArchiveInstant,
    OpenMeteoArchiveTimeAdapter, PublicStationBackfillDate, PublicStationBackfillDeadlines,
    PublicStationBackfillOperationError, PublicStationBackfillTimeAdapter, TaskFailure,
    TempestBackfillDate, TempestBackfillDeadlines, TempestBackfillOperationError,
    TempestBackfillTimeAdapter, TideBackfillDate, TideBackfillDeadlines,
    TideBackfillOperationError, TideBackfillTimeAdapter,
};

use crate::native_open_meteo_values::load_named_timezone;
use crate::{
    NativeWirePrimitives, PublicStationStartupTime, plan_worker_ingestion_deadlines,
    system_worker_health_milliseconds,
};

const CLOCK_INVALID: &str = "native_backfill_clock_invalid";
const DATE_INVALID: &str = "native_backfill_date_invalid";
const TIMEZONE_INVALID: &str = "native_backfill_timezone_invalid";
const QUARTER_HOUR_SECONDS: i32 = 15 * 60;
const MAXIMUM_LOCAL_MIDNIGHT_OFFSET_SECONDS: i32 = 14 * 60 * 60;

/// native calendar and deadline owner for every retained backfill family
#[derive(Debug)]
pub struct NativePublicBackfillTime {
    controls: ProviderControls,
    now: fn() -> Option<i64>,
    timezone: Option<CachedTimeZone>,
}

/// one retained host timezone
#[derive(Debug)]
struct CachedTimeZone {
    name: String,
    value: jiff::tz::TimeZone,
}

impl NativePublicBackfillTime {
    /// bind production provider controls to the process clock
    pub const fn new(controls: ProviderControls) -> Self {
        Self {
            controls,
            now: system_worker_health_milliseconds,
            timezone: None,
        }
    }

    /// sample one supported process instant
    fn now_milliseconds(&self) -> Result<i64, TaskFailure> {
        (self.now)().ok_or_else(|| failure(CLOCK_INVALID))
    }

    /// derive the current UTC calendar date
    fn admitted_today(&self) -> Result<AdmittedUtcDate, TaskFailure> {
        let now = Timestamp::from_millisecond(self.now_milliseconds()?)
            .map_err(|_| failure(CLOCK_INVALID))?;
        admit_date(jiff::tz::TimeZone::UTC.to_datetime(now).date())
    }

    /// derive one deadline plan from one clock sample
    fn admitted_deadlines(&self) -> Result<PublicStationBackfillDeadlines, TaskFailure> {
        let started_at_ms = self.now_milliseconds()?;
        let plan = plan_worker_ingestion_deadlines(started_at_ms, self.controls)?;
        Ok(PublicStationBackfillDeadlines {
            provider_deadline_at_ms: plan.provider_deadline_at_ms,
            run_deadline_at_ms: plan.run_deadline_at_ms,
            started_at_ms,
        })
    }

    /// load one host timezone at most once per selected identity
    fn timezone(&mut self, name: &str) -> Option<&jiff::tz::TimeZone> {
        // reload only when a later command selects another timezone
        if self
            .timezone
            .as_ref()
            .is_none_or(|timezone| timezone.name != name)
        {
            self.timezone = Some(CachedTimeZone {
                name: name.to_owned(),
                value: load_named_timezone(name)?,
            });
        }
        self.timezone.as_ref().map(|timezone| &timezone.value)
    }

    /// inject one deterministic clock for ordinary unit coverage
    #[cfg(test)]
    const fn with_clock(controls: ProviderControls, now: fn() -> Option<i64>) -> Self {
        Self {
            controls,
            now,
            timezone: None,
        }
    }
}

impl Default for NativePublicBackfillTime {
    /// use the original provider retry defaults
    fn default() -> Self {
        Self::new(ProviderControls::default())
    }
}

impl PublicStationBackfillTimeAdapter for NativePublicBackfillTime {
    /// return the live UTC date
    fn today_utc(&mut self) -> Result<PublicStationBackfillDate, TaskFailure> {
        self.admitted_today().map(public_station_date)
    }

    /// add exact UTC calendar days
    fn add_utc_days(
        &mut self,
        date: &PublicStationBackfillDate,
        days: i64,
    ) -> Result<PublicStationBackfillDate, TaskFailure> {
        checked_date_add(date.label(), date.start_ms(), days).map(public_station_date)
    }

    /// sample the provider and run deadlines once
    fn deadlines(
        &mut self,
    ) -> Result<PublicStationBackfillDeadlines, PublicStationBackfillOperationError> {
        self.admitted_deadlines()
            .map_err(|error| PublicStationBackfillOperationError::unexpected(error.code))
    }
}

impl PublicStationStartupTime for NativePublicBackfillTime {
    /// sample the startup diagnostic clock
    fn diagnostic_now(&mut self) -> i64 {
        (self.now)().unwrap_or_default()
    }
}

impl TempestBackfillTimeAdapter for NativePublicBackfillTime {
    /// return the live UTC date
    fn today_utc(&mut self) -> Result<TempestBackfillDate, TaskFailure> {
        self.admitted_today().map(public_station_date)
    }

    /// add exact UTC calendar days
    fn add_utc_days(
        &mut self,
        date: &TempestBackfillDate,
        days: i64,
    ) -> Result<TempestBackfillDate, TaskFailure> {
        checked_date_add(date.label(), date.start_ms(), days).map(public_station_date)
    }

    /// sample the provider and run deadlines once
    fn deadlines(&mut self) -> Result<TempestBackfillDeadlines, TempestBackfillOperationError> {
        self.admitted_deadlines()
            .map_err(|error| TempestBackfillOperationError::unexpected(error.code))
    }
}

impl TideBackfillTimeAdapter for NativePublicBackfillTime {
    /// return the live UTC date
    fn today_utc(&mut self) -> Result<TideBackfillDate, TaskFailure> {
        self.admitted_today().map(tide_date)
    }

    /// add exact UTC calendar days
    fn add_utc_days(
        &mut self,
        date: &TideBackfillDate,
        days: i64,
    ) -> Result<TideBackfillDate, TaskFailure> {
        checked_date_add(date.label(), date.start_ms(), days).map(tide_date)
    }

    /// sample the provider and run deadlines once
    fn deadlines(&mut self) -> Result<TideBackfillDeadlines, TideBackfillOperationError> {
        self.admitted_deadlines()
            .map(|deadlines| TideBackfillDeadlines {
                provider_deadline_at_ms: deadlines.provider_deadline_at_ms,
                run_deadline_at_ms: deadlines.run_deadline_at_ms,
                started_at_ms: deadlines.started_at_ms,
            })
            .map_err(|error| TideBackfillOperationError::unexpected(error.code))
    }
}

impl HistoricalBackfillCommandTimeAdapter for NativePublicBackfillTime {
    /// return the live UTC command date
    fn today_utc(&mut self) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
        self.admitted_today().map(command_date)
    }

    /// admit one strict UTC command date
    fn admit_utc_date(
        &mut self,
        label: &str,
    ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
        parse_date(label).and_then(admit_date).map(command_date)
    }

    /// add exact UTC command days
    fn add_utc_days(
        &mut self,
        date: &HistoricalBackfillCommandDate,
        days: i64,
    ) -> Result<HistoricalBackfillCommandDate, TaskFailure> {
        checked_date_add(date.label(), date.start_ms(), days).map(command_date)
    }
}

impl OpenMeteoArchiveTimeAdapter for NativePublicBackfillTime {
    /// add exact site calendar days
    fn add_calendar_days(&mut self, date: &str, days: u64) -> Result<String, TaskFailure> {
        let days = i64::try_from(days).map_err(|_| failure(DATE_INVALID))?;
        let date = parse_date(date)?;
        let span = Span::new()
            .try_days(days)
            .map_err(|_| failure(DATE_INVALID))?;
        date.checked_add(span)
            .map(|date| date.to_string())
            .map_err(|_| failure(DATE_INVALID))
    }

    /// resolve one exact site midnight
    fn local_midnight_utc(
        &mut self,
        date: &str,
        timezone: &str,
    ) -> Result<OpenMeteoArchiveInstant, TaskFailure> {
        let date = parse_date(date)?;
        let timezone = self
            .timezone(timezone)
            .ok_or_else(|| failure(TIMEZONE_INVALID))?;
        let local_midnight = date.at(0, 0, 0, 0);
        let mut candidates = Vec::with_capacity(2);

        // retain exact modern quarter-hour mappings from both ambiguity choices
        for candidate in [
            timezone.to_ambiguous_timestamp(local_midnight).earlier(),
            timezone.to_ambiguous_timestamp(local_midnight).later(),
        ] {
            let Ok(candidate) = candidate else {
                continue;
            };
            // reject gap normalization and offsets outside the original scan
            let offset_seconds = timezone.to_offset(candidate).seconds();
            if timezone.to_datetime(candidate) != local_midnight
                || offset_seconds.abs() > MAXIMUM_LOCAL_MIDNIGHT_OFFSET_SECONDS
                || offset_seconds % QUARTER_HOUR_SECONDS != 0
            {
                continue;
            }
            candidates.push(candidate);
        }
        // select the first instant from the predecessor's ascending offset scan
        let timestamp = candidates
            .into_iter()
            .max()
            .ok_or_else(|| failure(TIMEZONE_INVALID))?;
        let epoch_milliseconds = timestamp.as_millisecond();
        let utc = NativeWirePrimitives
            .format_utc_milliseconds(epoch_milliseconds)
            .ok_or_else(|| failure(TIMEZONE_INVALID))?;
        Ok(OpenMeteoArchiveInstant {
            epoch_milliseconds,
            utc,
        })
    }

    /// sample the provider and run deadlines once
    fn deadlines(&mut self) -> Result<TideBackfillDeadlines, TideBackfillOperationError> {
        self.admitted_deadlines()
            .map(|deadlines| TideBackfillDeadlines {
                provider_deadline_at_ms: deadlines.provider_deadline_at_ms,
                run_deadline_at_ms: deadlines.run_deadline_at_ms,
                started_at_ms: deadlines.started_at_ms,
            })
            .map_err(|error| TideBackfillOperationError::unexpected(error.code))
    }
}

/// one strict UTC calendar date
#[derive(Clone, Debug, Eq, PartialEq)]
struct AdmittedUtcDate {
    label: String,
    start_ms: i64,
}

/// parse one canonical Gregorian label
fn parse_date(label: &str) -> Result<Date, TaskFailure> {
    let date = label.parse::<Date>().map_err(|_| failure(DATE_INVALID))?;
    // reject normalized or expanded labels
    if date.to_string() != label {
        return Err(failure(DATE_INVALID));
    }
    Ok(date)
}

/// admit one date and its UTC midnight
fn admit_date(date: Date) -> Result<AdmittedUtcDate, TaskFailure> {
    let timestamp = jiff::tz::TimeZone::UTC
        .to_timestamp(date.at(0, 0, 0, 0))
        .map_err(|_| failure(DATE_INVALID))?;
    Ok(AdmittedUtcDate {
        label: date.to_string(),
        start_ms: timestamp.as_millisecond(),
    })
}

/// add days after validating the supplied witness
fn checked_date_add(label: &str, start_ms: i64, days: i64) -> Result<AdmittedUtcDate, TaskFailure> {
    let date = parse_date(label)?;
    let admitted = admit_date(date)?;
    // reject a label and epoch from different dates
    if admitted.start_ms != start_ms {
        return Err(failure(DATE_INVALID));
    }
    let span = Span::new()
        .try_days(days)
        .map_err(|_| failure(DATE_INVALID))?;
    admitted_date_add(date, span)
}

/// perform one checked civil-date addition
fn admitted_date_add(date: Date, span: Span) -> Result<AdmittedUtcDate, TaskFailure> {
    date.checked_add(span)
        .map_err(|_| failure(DATE_INVALID))
        .and_then(admit_date)
}

/// project one shared date into public-station storage
fn public_station_date(date: AdmittedUtcDate) -> PublicStationBackfillDate {
    PublicStationBackfillDate::admitted(date.label, date.start_ms)
}

/// project one shared date into tide storage
fn tide_date(date: AdmittedUtcDate) -> TideBackfillDate {
    TideBackfillDate::admitted(date.label, date.start_ms)
}

/// project one shared date into command storage
fn command_date(date: AdmittedUtcDate) -> HistoricalBackfillCommandDate {
    HistoricalBackfillCommandDate::admitted(date.label, date.start_ms)
}

/// construct one pre-provider time failure
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_709_294_400_000;

    /// return one fixed leap-day-adjacent instant
    fn fixed_now() -> Option<i64> {
        Some(NOW_MS)
    }

    /// bind the fixed clock to production retry controls
    fn time() -> NativePublicBackfillTime {
        NativePublicBackfillTime::with_clock(ProviderControls::default(), fixed_now)
    }

    /// preserve leap-day UTC arithmetic across all worker adapters
    #[test]
    fn shares_strict_utc_calendar_across_backfills() {
        let mut time = time();
        let today = PublicStationBackfillTimeAdapter::today_utc(&mut time).unwrap();
        assert_eq!(
            (today.label(), today.start_ms()),
            ("2024-03-01", 1_709_251_200_000)
        );
        let yesterday =
            PublicStationBackfillTimeAdapter::add_utc_days(&mut time, &today, -1).unwrap();
        assert_eq!(
            (yesterday.label(), yesterday.start_ms()),
            ("2024-02-29", 1_709_164_800_000)
        );

        let tide_today = TideBackfillTimeAdapter::today_utc(&mut time).unwrap();
        let tide_tomorrow =
            TideBackfillTimeAdapter::add_utc_days(&mut time, &tide_today, 1).unwrap();
        assert_eq!(
            (tide_tomorrow.label(), tide_tomorrow.start_ms()),
            ("2024-03-02", 1_709_337_600_000)
        );
        assert_eq!(PublicStationStartupTime::diagnostic_now(&mut time), NOW_MS);
    }

    /// preserve the default retry budget and finalization grace
    #[test]
    fn plans_original_default_deadlines() {
        let mut time = time();
        let deadlines = PublicStationBackfillTimeAdapter::deadlines(&mut time).unwrap();
        assert_eq!(
            deadlines,
            PublicStationBackfillDeadlines {
                provider_deadline_at_ms: NOW_MS + 90_000,
                run_deadline_at_ms: NOW_MS + 120_000,
                started_at_ms: NOW_MS,
            }
        );
    }

    /// reject rollover labels and mismatched date witnesses
    #[test]
    fn rejects_invalid_calendar_inputs() {
        let mut time = time();
        assert_eq!(
            HistoricalBackfillCommandTimeAdapter::admit_utc_date(&mut time, "2024-02-30"),
            Err(failure(DATE_INVALID))
        );
        let forged = PublicStationBackfillDate::admitted("2024-03-01", 0);
        assert_eq!(
            PublicStationBackfillTimeAdapter::add_utc_days(&mut time, &forged, 1),
            Err(failure(DATE_INVALID))
        );
    }

    /// preserve site-local midnight across daylight-saving transitions
    #[test]
    fn resolves_named_timezone_midnights() {
        let mut time = time();
        let spring = OpenMeteoArchiveTimeAdapter::local_midnight_utc(
            &mut time,
            "2024-03-10",
            "America/Los_Angeles",
        )
        .unwrap();
        let fall = OpenMeteoArchiveTimeAdapter::local_midnight_utc(
            &mut time,
            "2024-11-03",
            "America/Los_Angeles",
        )
        .unwrap();
        assert_eq!(spring.utc, "2024-03-10T08:00:00.000Z");
        assert_eq!(fall.utc, "2024-11-03T07:00:00.000Z");
        assert_eq!(
            OpenMeteoArchiveTimeAdapter::add_calendar_days(&mut time, "2024-02-28", 2).as_deref(),
            Ok("2024-03-01")
        );
    }
}
