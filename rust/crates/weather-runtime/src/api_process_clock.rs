//! Use one wall-clock sample for API startup and one for each admitted request.

use weather_api::{
    CanonicalInstant, CurrentTimeResolver, ServiceClockError, WeatherApiStartupClock,
    WeatherApiStartupPortError, WeatherServiceClock,
};
use weather_contracts::JsDateOrString;

use crate::system_worker_health_milliseconds;

/// Convert process wall-clock milliseconds through the existing canonical time owner.
pub struct WeatherApiProcessClock<Time, Now = fn() -> Option<i64>> {
    time: Time,
    now: Now,
}

impl<Time> WeatherApiProcessClock<Time> {
    /// Bind the ordinary process wall clock without sampling it during construction.
    pub const fn current_process(time: Time) -> Self {
        Self::new(time, system_worker_health_milliseconds)
    }
}

impl<Time, Now> WeatherApiProcessClock<Time, Now> {
    /// Bind the admitted time resolver and a clock used only for real observations.
    pub const fn new(time: Time, now: Now) -> Self {
        Self { time, now }
    }
}

impl<Time: CurrentTimeResolver, Now: FnMut() -> Option<i64>> WeatherApiProcessClock<Time, Now> {
    /// Resolve the exact captured milliseconds without consulting a second clock.
    fn capture(&mut self) -> Option<CanonicalInstant> {
        let epoch = (self.now)()?;
        self.time
            .resolve(&JsDateOrString::DateEpochMilliseconds(epoch))
            .ok()
            .filter(|instant| instant.epoch_milliseconds == epoch)
    }
}

impl<Time: CurrentTimeResolver, Now: FnMut() -> Option<i64>> WeatherApiStartupClock
    for WeatherApiProcessClock<Time, Now>
{
    /// Stamp all selected model runtimes with one shared startup instant.
    fn capture_loaded_at(&mut self) -> Result<CanonicalInstant, WeatherApiStartupPortError> {
        self.capture().ok_or(WeatherApiStartupPortError)
    }
}

impl<Time: CurrentTimeResolver, Now: FnMut() -> Option<i64>> WeatherServiceClock
    for WeatherApiProcessClock<Time, Now>
{
    /// Stamp one read request without changing its optional nominal data time.
    fn request_time(&mut self) -> Result<CanonicalInstant, ServiceClockError> {
        self.capture().ok_or(ServiceClockError)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use weather_api::CurrentProjectionError;

    use super::*;

    const STARTUP: i64 = 1_787_227_200_000;

    struct Time<'a>(&'a Cell<usize>);

    impl CurrentTimeResolver for Time<'_> {
        /// Admit the exact fixture time samples through their typed date shape.
        fn resolve(
            &self,
            value: &JsDateOrString,
        ) -> Result<CanonicalInstant, CurrentProjectionError> {
            self.0.set(self.0.get() + 1);
            let (epoch_milliseconds, utc) = match value {
                JsDateOrString::DateEpochMilliseconds(STARTUP) => {
                    (STARTUP, "2026-08-20T12:00:00.000Z")
                }
                JsDateOrString::DateEpochMilliseconds(epoch) if *epoch == STARTUP + 1 => {
                    (*epoch, "2026-08-20T12:00:00.001Z")
                }
                _ => panic!("unknown fixture clock instant"),
            };
            Ok(CanonicalInstant {
                epoch_milliseconds,
                utc: utc.to_owned(),
            })
        }
    }

    /// Keep startup and request observations independent and skip conversion when no sample exists.
    #[test]
    fn shares_admitted_time_owner_without_repeating_the_wall_clock_sample() {
        let conversions = Cell::new(0);
        let mut samples = [Some(STARTUP), Some(STARTUP + 1), None].into_iter();
        let mut clock = WeatherApiProcessClock::new(
            Time(&conversions),
            // fail on an extra wall-clock observation
            || samples.next().expect("expected observation"),
        );

        assert_eq!(
            clock.capture_loaded_at().unwrap().epoch_milliseconds,
            STARTUP
        );
        assert_eq!(
            clock.request_time().unwrap().epoch_milliseconds,
            STARTUP + 1
        );
        assert_eq!(clock.request_time(), Err(ServiceClockError));
        assert_eq!(conversions.get(), 2);
    }
}
