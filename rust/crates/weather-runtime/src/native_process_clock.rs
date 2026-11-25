//! Concrete shared process clock and civil-time adapters for retained owners.

use std::fmt::{Display, Formatter};

use weather_api::CanonicalInstant;
use weather_database::AdmittedRevisionUtcInstant;
use weather_edge::{
    AdmittedForecastAdjustmentScorecardNow, ArchiveWirePrimitives,
    ForecastAdjustmentScorecardClock, RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec,
};
use weather_worker::{
    RainCollectionClock, RainCollectionTimeAdapter, RainCollectionTimeValue, TaskFailure,
};

use crate::{
    NativeWirePrimitives, RainRepositoryTimeBoundary, ScorecardClock, WorkerIterationTime,
    system_worker_health_milliseconds,
};

/// One real wall-clock owner shared by the worker and immutable archive paths.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeProcessClock;

impl RevisionArchiveCanonicalClock for NativeProcessClock {
    /// Sample once before formatting the new immutable receipt instant.
    fn canonical_now(&mut self) -> Option<String> {
        NativeWirePrimitives.format_utc_milliseconds(system_worker_health_milliseconds()?)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl weather_edge::ForecastArchiveLockClock for NativeProcessClock {
    /// Sample the same real wall clock for a retained lease expiry.
    fn now_epoch_milliseconds(&mut self) -> Option<i64> {
        system_worker_health_milliseconds()
    }
}

impl RainCollectionClock for NativeProcessClock {
    /// Admit exactly one planning instant or preserve the clock failure.
    fn now(&mut self) -> RainCollectionTimeValue {
        system_worker_health_milliseconds().map_or(
            RainCollectionTimeValue::Invalid,
            RainCollectionTimeValue::EpochMilliseconds,
        )
    }
}

impl ForecastAdjustmentScorecardClock for NativeProcessClock {
    type Error = NativeProcessClockError;

    /// Bind the scorecard's canonical instant to the same epoch sample.
    fn now(&self) -> Result<AdmittedForecastAdjustmentScorecardNow, Self::Error> {
        let epoch_ms = system_worker_health_milliseconds().ok_or(NativeProcessClockError)?;
        let canonical = NativeWirePrimitives
            .format_utc_milliseconds(epoch_ms)
            .ok_or(NativeProcessClockError)?;
        Ok(AdmittedForecastAdjustmentScorecardNow {
            canonical,
            epoch_ms,
        })
    }
}

/// An unavailable or non-representable wall-clock sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeProcessClockError;

impl Display for NativeProcessClockError {
    /// Retain a bounded failure for the worker and database owners.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid canonical UTC instant")
    }
}

impl std::error::Error for NativeProcessClockError {}

impl WorkerIterationTime for NativeWirePrimitives {
    type Error = NativeProcessClockError;

    /// Bind already sampled worker time to the canonical database timestamp.
    fn utc_milliseconds(
        &mut self,
        epoch_milliseconds: i64,
    ) -> Result<AdmittedRevisionUtcInstant, Self::Error> {
        self.format_utc_milliseconds(epoch_milliseconds)
            .map(AdmittedRevisionUtcInstant::admitted)
            .ok_or(NativeProcessClockError)
    }
}

impl RainRepositoryTimeBoundary for NativeWirePrimitives {
    type Error = NativeProcessClockError;

    /// Admit the retained rain timestamp only at the canonical wire precision.
    fn admit_rain_instant(&mut self, value: &str) -> Result<CanonicalInstant, Self::Error> {
        let epoch_milliseconds = self
            .canonical_utc_milliseconds(value)
            .ok_or(NativeProcessClockError)?;
        Ok(CanonicalInstant {
            epoch_milliseconds,
            utc: value.to_owned(),
        })
    }
}

impl ScorecardClock for NativeWirePrimitives {
    /// Admit the original strict UTC receipt timestamp.
    fn utc_epoch_milliseconds(&self, value: &str) -> Option<i64> {
        self.canonical_utc_milliseconds(value)
    }

    /// Format the canonical scorecard interval endpoint.
    fn canonical_utc(&self, milliseconds: i64) -> Option<String> {
        self.format_utc_milliseconds(milliseconds)
    }
}

impl RainCollectionTimeAdapter for NativeWirePrimitives {
    /// Format the collection slot at the original millisecond UTC precision.
    fn canonical_utc_iso(&mut self, epoch_ms: i64) -> Result<String, TaskFailure> {
        self.format_utc_milliseconds(epoch_ms)
            .ok_or_else(|| TaskFailure {
                attempts: 0,
                code: "rain_collection_clock_invalid".to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that an actual receipt and scorecard use the same native timestamp contract.
    #[test]
    fn native_process_clock_round_trips_current_receipt() {
        let mut clock = NativeProcessClock;
        let before = system_worker_health_milliseconds().unwrap();
        let canonical = clock.canonical_now().unwrap();
        let receipt = NativeWirePrimitives
            .canonical_utc_milliseconds(&canonical)
            .unwrap();
        let scorecard = ForecastAdjustmentScorecardClock::now(&clock).unwrap();
        let after = system_worker_health_milliseconds().unwrap();
        assert!((before..=after).contains(&receipt));
        assert!((before..=after).contains(&scorecard.epoch_ms));
        assert_eq!(
            NativeWirePrimitives.canonical_utc_milliseconds(&scorecard.canonical),
            Some(scorecard.epoch_ms)
        );
        assert_eq!(
            NativeWirePrimitives.canonical_utc_iso(0).as_deref(),
            Ok("1970-01-01T00:00:00.000Z")
        );
    }
}
