//! Admit one complete causal temperature recent-error receipt before numerical inference.

use std::collections::HashSet;

use weather_contracts::{
    JsDateOrString, Nullable, TemperatureMosRuntimeRawReason, TemperatureMosRuntimeRecentErrorState,
};

use crate::{CanonicalInstant, CurrentTimeResolver};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const SOURCE_DELAY_MILLISECONDS: i64 = 7 * HOUR_MILLISECONDS;
const SIX_HOURS_MILLISECONDS: i64 = 6 * HOUR_MILLISECONDS;

type RawReason = TemperatureMosRuntimeRawReason;

/// Validate the exact per-run recent-error state without parsing clocks locally.
pub(crate) fn validate_mos_recent_error_state(
    state: &TemperatureMosRuntimeRecentErrorState,
    run: &CanonicalInstant,
    resolver: &dyn CurrentTimeResolver,
) -> Result<(), TemperatureMosRuntimeRawReason> {
    let target_run = resolve_wire_instant(&state.targetRunInitializedAt, resolver)?;
    // bind the retained state to this exact initialized model run
    if target_run.epoch_milliseconds != run.epoch_milliseconds {
        return Err(RawReason::RecentErrorStateRunMismatch);
    }

    let expected_window_end = run
        .epoch_milliseconds
        .checked_sub(SOURCE_DELAY_MILLISECONDS)
        .ok_or(RawReason::InvalidRecentErrorState)?;
    let window_end = resolve_wire_instant(&state.windowEndValidAt, resolver)?;
    // preserve the frozen seven-hour observation boundary
    if window_end.epoch_milliseconds != expected_window_end {
        return Err(RawReason::RecentErrorStateAsOfMismatch);
    }

    // retain the original bounded integer support relationships
    if state.n24 > 24 || state.n72 < state.n24 || state.n72 > 72 || state.localDates > state.n72 {
        return Err(RawReason::InvalidRecentErrorState);
    }

    let expected_keys =
        usize::try_from(state.n72).map_err(|_| RawReason::InvalidRecentErrorState)?;
    let mut source_keys = HashSet::with_capacity(expected_keys);
    // bind one unique nonempty audit key to every selected source hour
    if state.sourceKeys.len() != expected_keys
        || state
            .sourceKeys
            .iter()
            .any(|key| key.is_empty() || !source_keys.insert(key.as_str()))
    {
        return Err(RawReason::InvalidRecentErrorState);
    }

    let short_supported = state.n24 >= 6;
    let long_supported = state.n72 >= 24;
    let expected_supported = short_supported && long_supported && state.localDates >= 2;
    // recompute the exact source support gate rather than trusting its flag
    if state.supported != expected_supported {
        return Err(RawReason::InvalidRecentErrorState);
    }

    // require a bounded short statistic exactly when short support exists
    if !bias_matches_support(short_supported, &state.b24C) {
        return Err(RawReason::InvalidRecentErrorState);
    }
    // require both bounded long statistics exactly when long support exists
    if !bias_matches_support(long_supported, &state.b72C)
        || !mad_matches_support(long_supported, &state.mad72C)
    {
        return Err(RawReason::InvalidRecentErrorState);
    }

    // empty state must not retain causal maxima from another run
    if state.n72 == 0
        && (!matches!(state.maximumSourceValidAt, Nullable::Null)
            || !matches!(state.maximumSourceRunInitializedAt, Nullable::Null))
    {
        return Err(RawReason::InvalidRecentErrorState);
    }

    // only populated state has further maximum chronology to validate
    if state.n72 == 0 {
        return Ok(());
    }
    let (Nullable::Value(maximum_valid_wire), Nullable::Value(maximum_run_wire)) = (
        &state.maximumSourceValidAt,
        &state.maximumSourceRunInitializedAt,
    ) else {
        return Err(RawReason::InvalidRecentErrorState);
    };
    let maximum_valid = resolve_wire_instant(maximum_valid_wire, resolver)?;
    let maximum_run = resolve_wire_instant(maximum_run_wire, resolver)?;
    let long_window_start = expected_window_end
        .checked_sub(71 * HOUR_MILLISECONDS)
        .ok_or(RawReason::InvalidRecentErrorState)?;
    let short_window_start = expected_window_end
        .checked_sub(23 * HOUR_MILLISECONDS)
        .ok_or(RawReason::InvalidRecentErrorState)?;

    // reject observations or source runs unavailable at initialization
    if maximum_valid.epoch_milliseconds > expected_window_end
        || maximum_run.epoch_milliseconds >= run.epoch_milliseconds
    {
        return Err(RawReason::RecentErrorStateContainsFutureData);
    }

    let valid_age = expected_window_end
        .checked_sub(maximum_valid.epoch_milliseconds)
        .ok_or(RawReason::RecentErrorStateOutsideWindow)?;
    // bind the maximum to the inclusive hourly short and long windows
    if maximum_valid.epoch_milliseconds < long_window_start
        || (state.n24 > 0 && maximum_valid.epoch_milliseconds < short_window_start)
        || valid_age.rem_euclid(HOUR_MILLISECONDS) != 0
    {
        return Err(RawReason::RecentErrorStateOutsideWindow);
    }

    // require one exact UTC six-hour ECMWF source initialization boundary
    if maximum_run
        .epoch_milliseconds
        .rem_euclid(SIX_HOURS_MILLISECONDS)
        != 0
    {
        return Err(RawReason::InvalidRecentErrorState);
    }

    let earliest_source_run = maximum_valid
        .epoch_milliseconds
        .checked_sub(18 * HOUR_MILLISECONDS)
        .ok_or(RawReason::InvalidRecentErrorState)?;
    let latest_source_run = maximum_valid
        .epoch_milliseconds
        .checked_sub(7 * HOUR_MILLISECONDS)
        .ok_or(RawReason::InvalidRecentErrorState)?;
    // enforce the original inclusive source-lead range across both maxima
    if maximum_run.epoch_milliseconds < earliest_source_run
        || maximum_run.epoch_milliseconds > latest_source_run
    {
        return Err(RawReason::RecentErrorStateSourceRunMismatch);
    }
    Ok(())
}

/// Resolve one exact canonical wire instant through the injected calendar owner.
fn resolve_wire_instant(
    value: &str,
    resolver: &dyn CurrentTimeResolver,
) -> Result<CanonicalInstant, RawReason> {
    let resolved = resolver
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .map_err(|_| RawReason::InvalidRecentErrorState)?;
    // reject aliases or normalization even when they resolve to the same epoch
    if resolved.utc != value {
        return Err(RawReason::InvalidRecentErrorState);
    }
    Ok(resolved)
}

/// Match one capped bias to the exact present-versus-null support state.
fn bias_matches_support(supported: bool, value: &Nullable<f64>) -> bool {
    match (supported, value) {
        (true, Nullable::Value(value)) => value.is_finite() && value.abs() <= 6.0,
        (false, Nullable::Null) => true,
        _ => false,
    }
}

/// Match one capped MAD to the exact present-versus-null support state.
fn mad_matches_support(supported: bool, value: &Nullable<f64>) -> bool {
    match (supported, value) {
        (true, Nullable::Value(value)) => value.is_finite() && (0.0..=6.0).contains(value),
        (false, Nullable::Null) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use weather_contracts::TemperatureMosRuntimeRecentErrorStateCohortLiteral;

    use super::*;
    use crate::CurrentProjectionError;

    const RUN: &str = "2026-01-01T12:00:00.000Z";
    const WINDOW: &str = "2026-01-01T05:00:00.000Z";
    const SOURCE_RUN: &str = "2025-12-31T18:00:00.000Z";

    struct FixtureResolver;

    impl CurrentTimeResolver for FixtureResolver {
        /// Resolve only the three canonical fixture clocks.
        fn resolve(
            &self,
            value: &JsDateOrString,
        ) -> Result<CanonicalInstant, CurrentProjectionError> {
            let JsDateOrString::String(value) = value else {
                return Err(clock_error());
            };
            let value = String::from_utf16(value).map_err(|_| clock_error())?;
            let epoch_milliseconds = match value.as_str() {
                RUN => 1_767_268_800_000,
                WINDOW => 1_767_243_600_000,
                SOURCE_RUN => 1_767_204_000_000,
                _ => return Err(clock_error()),
            };
            Ok(CanonicalInstant {
                epoch_milliseconds,
                utc: value,
            })
        }
    }

    /// Return one populated but unsupported causal receipt.
    fn populated_unsupported_state() -> TemperatureMosRuntimeRecentErrorState {
        TemperatureMosRuntimeRecentErrorState {
            b24C: Nullable::Null,
            b72C: Nullable::Null,
            cohort: TemperatureMosRuntimeRecentErrorStateCohortLiteral,
            localDates: 0,
            mad72C: Nullable::Null,
            maximumSourceRunInitializedAt: Nullable::Value(SOURCE_RUN.to_owned()),
            maximumSourceValidAt: Nullable::Value(WINDOW.to_owned()),
            n24: 1,
            n72: 1,
            sourceKeys: vec!["source-hour".to_owned()],
            supported: false,
            targetRunInitializedAt: RUN.to_owned(),
            windowEndValidAt: WINDOW.to_owned(),
        }
    }

    /// Return the already admitted target run fixture.
    fn run() -> CanonicalInstant {
        CanonicalInstant {
            epoch_milliseconds: 1_767_268_800_000,
            utc: RUN.to_owned(),
        }
    }

    /// Produce a bounded fixture-only resolver failure.
    fn clock_error() -> CurrentProjectionError {
        CurrentProjectionError {
            code: "fixture.clock",
            detail: "unknown fixture clock".to_owned(),
        }
    }

    #[test]
    fn refuses_run_before_inspecting_window() {
        let mut state = populated_unsupported_state();
        state.targetRunInitializedAt = SOURCE_RUN.to_owned();
        state.windowEndValidAt = "not-a-clock".to_owned();
        assert_eq!(
            validate_mos_recent_error_state(&state, &run(), &FixtureResolver),
            Err(RawReason::RecentErrorStateRunMismatch),
        );
    }

    #[test]
    fn refuses_mismatched_seven_hour_window() {
        let mut state = populated_unsupported_state();
        state.windowEndValidAt = SOURCE_RUN.to_owned();
        assert_eq!(
            validate_mos_recent_error_state(&state, &run(), &FixtureResolver),
            Err(RawReason::RecentErrorStateAsOfMismatch),
        );
    }

    #[test]
    fn admits_populated_unsupported_state() {
        assert_eq!(
            validate_mos_recent_error_state(
                &populated_unsupported_state(),
                &run(),
                &FixtureResolver,
            ),
            Ok(()),
        );
    }
}
