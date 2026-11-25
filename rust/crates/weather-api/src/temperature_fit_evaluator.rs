//! Typed, authority-free pure MOS evaluation for isolated historical fit rows.

use crate::maintenance_temperature_model_causality::validate_mos_model_causality;
use crate::maintenance_temperature_recent_error_admission::validate_mos_recent_error_state;
use crate::{CanonicalInstant, CurrentTimeResolver, ForecastTemperatureMosAdmissionBoundary};
use weather_adjustment::{
    AdmittedTemperatureMosApplicationInput, AdmittedTemperatureMosCalendar,
    BorrowedTemperatureMosServingModel, apply_admitted_temperature_mos_forecast,
};
use weather_contracts::{
    JsDateOrString, Nullable, TemperatureMosRuntimeForecastModelCycleEnum,
    TemperatureMosRuntimeInput, TemperatureMosRuntimeModelMaterialScopeEnum,
    TemperatureMosRuntimeRawReason, TemperatureMosRuntimeResult,
};

const HOUR: i64 = 3_600_000;
const ERA_CUTOVER: i64 = 1_778_565_600_000;

/// Apply the source fitted model to a typed historical row without granting serving authority.
pub fn evaluate_temperature_fit_row(
    input: &TemperatureMosRuntimeInput,
    calendar: AdmittedTemperatureMosCalendar,
    time: &dyn CurrentTimeResolver,
    admission: &dyn ForecastTemperatureMosAdmissionBoundary,
) -> Result<TemperatureMosRuntimeResult, TemperatureMosRuntimeRawReason> {
    use TemperatureMosRuntimeRawReason as Error;
    let forecast = &input.forecast;
    let model = BorrowedTemperatureMosServingModel::from(&input.model);
    // retain original forecast and model-era checks before fitting chronology
    let run = canonical(time, &forecast.runInitializedAt)?;
    let valid = canonical(time, &forecast.validAt)?;
    let cycle = if run.epoch_milliseconds >= ERA_CUTOVER {
        TemperatureMosRuntimeForecastModelCycleEnum::Value50r1
    } else {
        TemperatureMosRuntimeForecastModelCycleEnum::Value49r1
    };
    if forecast.key.is_empty()
        || !forecast.rawTemperatureC.is_finite()
        || run.epoch_milliseconds.rem_euclid(6 * HOUR) != 0
        || forecast.modelCycle != cycle
        || nonfinite(&forecast.rawRelativeHumidityPercent)
        || nonfinite(&forecast.rawWindSpeedMps)
    {
        return Err(Error::InvalidForecast);
    }
    match model.scope() {
        TemperatureMosRuntimeModelMaterialScopeEnum::InitializationFirst12
            if !(1..=12).contains(&forecast.modelLeadHours) =>
        {
            return Err(Error::OutsideInitializationFirst12);
        }
        TemperatureMosRuntimeModelMaterialScopeEnum::AssumedDelay6Next12
            if !(7..=18).contains(&forecast.modelLeadHours) =>
        {
            return Err(Error::OutsideAssumedDelay6Next12);
        }
        _ => {}
    }
    let lead = i64::try_from(forecast.modelLeadHours)
        .ok()
        .and_then(|hours| hours.checked_mul(HOUR))
        .ok_or(Error::InvalidForecast)?;
    if valid.epoch_milliseconds.checked_sub(run.epoch_milliseconds) != Some(lead) {
        return Err(Error::InvalidForecast);
    }
    // reuse the existing source-identical model and recent-error causal validators
    validate_mos_model_causality(model, &run, &valid, time, admission)?;
    validate_mos_recent_error_state(&input.recentErrorState, &run, time)?;
    Ok(apply_admitted_temperature_mos_forecast(
        AdmittedTemperatureMosApplicationInput {
            calendar,
            model,
            model_cycle: forecast.modelCycle,
            model_lead_hours: forecast.modelLeadHours,
            raw_relative_humidity_percent: &forecast.rawRelativeHumidityPercent,
            raw_temperature_c: forecast.rawTemperatureC,
            raw_wind_speed_mps: &forecast.rawWindSpeedMps,
            recent_error_state: &input.recentErrorState,
        },
    ))
}

// retain canonical original initialized and valid clocks
fn canonical(
    time: &dyn CurrentTimeResolver,
    value: &str,
) -> Result<CanonicalInstant, TemperatureMosRuntimeRawReason> {
    let instant = time
        .resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .map_err(|_| TemperatureMosRuntimeRawReason::InvalidForecast)?;
    if instant.utc != value {
        return Err(TemperatureMosRuntimeRawReason::InvalidForecast);
    }
    Ok(instant)
}

// check the source finite optional weather values
fn nonfinite(value: &Nullable<f64>) -> bool {
    matches!(value,Nullable::Value(number) if !number.is_finite())
}
