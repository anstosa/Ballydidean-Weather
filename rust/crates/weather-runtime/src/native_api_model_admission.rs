//! Native request-time calendar and source admission for installed forecast models.

use weather_api::{
    AdmittedTemperatureMosCalendar, AdmittedWindRowCalendar, CanonicalInstant,
    CurrentProjectionError, CurrentTimeResolver, ForecastTemperatureMosAdmissionBoundary,
    MaintenanceWindModelCalendar,
};
use weather_contracts::{
    JsDateOrString, TemperatureCanarySourceForecast, TemperatureCanarySourceForecastModelCycleEnum,
};

use crate::{
    NativeApiTimeValues, NativePredecessorCalendarFeatures, NativePredecessorModelCalendar,
    NativeWirePrimitives, format_predecessor_source_utc, parse_predecessor_source_utc,
};

const HOUR_MS: i64 = 3_600_000;
const MODEL_50R1_CUTOVER_MILLISECONDS: i64 = 1_778_565_600_000;
const MONTH_STARTS: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

/// Shared native request values and the fixed model calendar admission owner.
#[derive(Clone)]
pub struct NativeApiModelAdmission {
    service_time: NativeApiTimeValues,
    wire: NativeWirePrimitives,
    calendar: NativePredecessorModelCalendar,
}

impl NativeApiModelAdmission {
    /// Admit the pinned predecessor model calendar while retaining the ordinary API zone cache.
    pub fn new(service_time: NativeApiTimeValues) -> Result<Self, CurrentProjectionError> {
        let calendar = NativePredecessorModelCalendar::new().ok_or_else(model_calendar_error)?;
        Ok(Self {
            service_time,
            wire: NativeWirePrimitives::new(),
            calendar,
        })
    }

    /// Construct the production owner with a fresh bounded API timezone cache.
    pub fn production() -> Result<Self, CurrentProjectionError> {
        Self::new(NativeApiTimeValues::default())
    }

    /// Borrow the same native API time owner for ordinary public service boundaries.
    pub const fn service_time_values(&self) -> &NativeApiTimeValues {
        &self.service_time
    }

    /// Resolve exact predecessor local fields for an admitted source instant.
    fn local(
        &self,
        instant: &CanonicalInstant,
    ) -> Result<NativePredecessorCalendarFeatures, CurrentProjectionError> {
        self.admitted_timestamp(instant)?;
        self.calendar.features(&instant.utc)
    }

    /// Confirm the exact source UTC spelling and epoch identity across the full model range.
    fn admitted_timestamp(
        &self,
        instant: &CanonicalInstant,
    ) -> Result<i64, CurrentProjectionError> {
        let parsed = parse_predecessor_source_utc(&instant.utc).ok_or_else(model_calendar_error)?;
        // reject a caller-constructed canonical pair whose two representations differ
        if parsed != *instant {
            return Err(model_calendar_error());
        }
        Ok(parsed.epoch_milliseconds)
    }

    /// Resolve one exact canonical UTC text through the retained native primitive.
    fn canonical(&self, value: &str) -> Result<CanonicalInstant, CurrentProjectionError> {
        self.resolve(&JsDateOrString::String(value.encode_utf16().collect()))
    }
}

impl CurrentTimeResolver for NativeApiModelAdmission {
    /// Delegate Date/string normalization to the approved native wire primitive.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        self.wire.resolve(value).or_else(|error| {
            // the predecessor supports the final four-digit UTC half-day outside Jiff's timestamp range
            match value {
                JsDateOrString::DateEpochMilliseconds(epoch) => {
                    format_predecessor_source_utc(*epoch)
                        .map(|utc| CanonicalInstant {
                            epoch_milliseconds: *epoch,
                            utc,
                        })
                        .ok_or(error)
                }
                JsDateOrString::String(units) => String::from_utf16(units)
                    .ok()
                    .and_then(|text| parse_predecessor_source_utc(&text))
                    .ok_or(error),
                JsDateOrString::InvalidDate => Err(error),
            }
        })
    }
}

impl MaintenanceWindModelCalendar for NativeApiModelAdmission {
    /// Derive the original local month, season, and daypart from the pinned predecessor calendar.
    fn calendar(&self, valid_at: &str) -> Result<AdmittedWindRowCalendar, CurrentProjectionError> {
        self.calendar.calendar(valid_at)
    }

    /// Admit only the generated and independently checked predecessor ICU and tzdata fingerprint.
    fn fingerprint_matches(&self, icu_version: &str, tzdata_version: &str) -> bool {
        self.calendar
            .fingerprint_matches(icu_version, tzdata_version)
    }
}

impl ForecastTemperatureMosAdmissionBoundary for NativeApiModelAdmission {
    /// Validate source-cycle identity and derive the exact frozen MOS harmonic bases.
    fn admit_calendar(
        &self,
        source: &TemperatureCanarySourceForecast,
        run_initialized_at: &CanonicalInstant,
        valid_at: &CanonicalInstant,
    ) -> Result<AdmittedTemperatureMosCalendar, CurrentProjectionError> {
        let run_timestamp = self.admitted_timestamp(run_initialized_at)?;
        self.admitted_timestamp(valid_at)?;
        let source_run = self.canonical(&source.runInitializedAt)?;
        let source_valid = self.canonical(&source.validAt)?;
        // bind the generated source strings to the already admitted causal clocks
        if source_run != *run_initialized_at || source_valid != *valid_at {
            return Err(model_calendar_error());
        }
        let expected_cycle =
            if run_initialized_at.epoch_milliseconds >= MODEL_50R1_CUTOVER_MILLISECONDS {
                TemperatureCanarySourceForecastModelCycleEnum::Value50r1
            } else {
                TemperatureCanarySourceForecastModelCycleEnum::Value49r1
            };
        // require one authentic six-hour initialization and matching ECMWF model era
        if run_initialized_at
            .epoch_milliseconds
            .rem_euclid(6 * HOUR_MS)
            != 0
            || source.modelCycle != expected_cycle
        {
            return Err(model_calendar_error());
        }

        let local = self.local(valid_at)?;
        let year = model_local_year(&local.local_date)?;
        let annual = std::f64::consts::TAU
            * (f64::from(day_of_year(&local, year)?) + f64::from(local.hour) / 24.0)
            / f64::from(days_in_year(year));
        let daily = std::f64::consts::TAU * f64::from(local.hour) / 24.0;
        let run_hour = run_timestamp.rem_euclid(24 * HOUR_MS) / HOUR_MS;
        let cycle = std::f64::consts::TAU
            * f64::from(u8::try_from(run_hour).map_err(|_| model_calendar_error())?)
            / 24.0;
        let calendar = AdmittedTemperatureMosCalendar {
            annual_basis: [
                annual.sin(),
                annual.cos(),
                (2.0 * annual).sin(),
                (2.0 * annual).cos(),
            ],
            cycle_basis: [cycle.sin(), cycle.cos()],
            daily_basis: [
                daily.sin(),
                daily.cos(),
                (2.0 * daily).sin(),
                (2.0 * daily).cos(),
            ],
        };
        // reject any platform math failure rather than feeding nonfinite model features
        if calendar
            .annual_basis
            .iter()
            .chain(calendar.cycle_basis.iter())
            .chain(calendar.daily_basis.iter())
            .any(|value| !value.is_finite())
        {
            return Err(model_calendar_error());
        }
        Ok(calendar)
    }

    /// Resolve the fixed site-local date and hour used by monthly scope and embargo checks.
    fn local_date_hour(
        &self,
        utc: &CanonicalInstant,
    ) -> Result<(String, u8), CurrentProjectionError> {
        let local = self.local(utc)?;
        Ok((local.local_date, local.hour))
    }
}

/// Preserve the source MOS day-of-year rejection for unpadded early-year ICU labels.
fn model_local_year(date: &str) -> Result<i16, CurrentProjectionError> {
    // the predecessor slices four characters before Date.parse; early-year labels are not valid there
    if date.len() != 10 {
        return Err(model_calendar_error());
    }
    date.get(..4)
        .and_then(|year| year.parse().ok())
        .ok_or_else(model_calendar_error)
}

/// Return the zero-based Gregorian day index used by the original MOS annual angle.
fn day_of_year(
    local: &NativePredecessorCalendarFeatures,
    year: i16,
) -> Result<u16, CurrentProjectionError> {
    let month_index = usize::from(
        local
            .month
            .checked_sub(1)
            .ok_or_else(model_calendar_error)?,
    );
    let mut day = *MONTH_STARTS
        .get(month_index)
        .ok_or_else(model_calendar_error)?
        + u16::from(
            local
                .local_day
                .checked_sub(1)
                .ok_or_else(model_calendar_error)?,
        );
    // insert leap day after February under the Gregorian rule
    if local.month > 2 && is_leap_year(year) {
        day = day.checked_add(1).ok_or_else(model_calendar_error)?;
    }
    Ok(day)
}

/// Return the exact Gregorian year length used by the original annual harmonics.
fn days_in_year(year: i16) -> u16 {
    if is_leap_year(year) { 366 } else { 365 }
}

/// Apply the Gregorian leap-year rule to one Jiff-admitted civil year.
fn is_leap_year(year: i16) -> bool {
    year % 400 == 0 || (year % 4 == 0 && year % 100 != 0)
}

/// Construct one bounded model-calendar failure without changing existing startup gates.
fn model_calendar_error() -> CurrentProjectionError {
    CurrentProjectionError {
        code: "api.model_calendar_invalid",
        detail: "forecast model calendar or source identity is invalid".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use weather_contracts::{
        ForecastAdjustmentCoefficientDaypartEnum, ForecastAdjustmentCoefficientSeasonEnum,
        Nullable, TemperatureCanarySourceForecastDatasetLiteral,
        TemperatureCanarySourceForecastProviderKeyLiteral,
        TemperatureCanarySourceForecastUpstreamModelLiteral,
    };

    use super::*;

    /// Derive real site calendar features under the pinned predecessor fingerprint.
    #[test]
    fn native_model_admission_derives_request_calendars() {
        let values = NativeApiModelAdmission::production().unwrap();
        let wind =
            MaintenanceWindModelCalendar::calendar(&values, "2026-09-12T21:00:00.000Z").unwrap();
        assert_eq!(wind.month, 9);
        assert_eq!(wind.season, ForecastAdjustmentCoefficientSeasonEnum::Autumn);
        assert_eq!(
            wind.daypart,
            ForecastAdjustmentCoefficientDaypartEnum::Afternoon
        );
        assert!(MaintenanceWindModelCalendar::fingerprint_matches(
            &values, "78.3", "2026b"
        ));
        assert!(!MaintenanceWindModelCalendar::fingerprint_matches(
            &values, "78.3", "2026f"
        ));

        let run = values.canonical("2026-09-12T18:00:00.000Z").unwrap();
        let valid = values.canonical("2026-09-12T21:00:00.000Z").unwrap();
        let source = temperature_source(&run.utc, &valid.utc);
        let calendar =
            ForecastTemperatureMosAdmissionBoundary::admit_calendar(&values, &source, &run, &valid)
                .unwrap();
        assert!(
            calendar
                .annual_basis
                .iter()
                .chain(calendar.cycle_basis.iter())
                .chain(calendar.daily_basis.iter())
                .all(|value| value.is_finite())
        );
        assert!((calendar.daily_basis[0] + 0.5).abs() < 1e-12);
        assert!((calendar.daily_basis[1] + 3.0_f64.sqrt() / 2.0).abs() < 1e-12);
        assert!((calendar.cycle_basis[0] + 1.0).abs() < 1e-12);
        assert_eq!(
            ForecastTemperatureMosAdmissionBoundary::local_date_hour(&values, &valid).unwrap(),
            ("2026-09-12".to_owned(), 14)
        );
    }

    /// Build one complete generated temperature source for ordinary calendar admission.
    fn temperature_source(run: &str, valid: &str) -> TemperatureCanarySourceForecast {
        TemperatureCanarySourceForecast {
            adapterVersion: "open-meteo-ecmwf-single-run/v1".to_owned(),
            dataset: TemperatureCanarySourceForecastDatasetLiteral,
            firstReceivedAt: "2026-09-12T18:10:00.000Z".to_owned(),
            modelCycle: TemperatureCanarySourceForecastModelCycleEnum::Value50r1,
            modelLeadHours: 3,
            providerKey: TemperatureCanarySourceForecastProviderKeyLiteral,
            providerResponseSha256: "a".repeat(64),
            rawRelativeHumidityPercent: Nullable::Value(70.0),
            rawTemperatureC: 10.0,
            rawWindSpeedMps: Nullable::Value(4.0),
            runInitializedAt: run.to_owned(),
            upstreamModel: TemperatureCanarySourceForecastUpstreamModelLiteral,
            validAt: valid.to_owned(),
        }
    }
}
