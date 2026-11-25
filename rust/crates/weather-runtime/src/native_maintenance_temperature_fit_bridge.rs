//! Native historical temperature inference bridge over the generated MOS input contracts.

use crate::{
    NativeApiModelAdmission, NativePredecessorModelCalendar, NativeWirePrimitives,
    maintenance_canonical_json::{field, text},
    native_maintenance_policy_comparison::{number, object, string},
    parse_predecessor_source_utc,
};
use weather_api::{AdmittedTemperatureMosCalendar, evaluate_temperature_fit_row};
use weather_contracts::{
    JsonValue, Nullable, TemperatureMosPermanentRuntimeModel,
    TemperatureMosPermanentRuntimeModelContractVersionLiteral, TemperatureMosRuntimeForecast,
    TemperatureMosRuntimeForecastCohortLiteral, TemperatureMosRuntimeForecastModelCycleEnum,
    TemperatureMosRuntimeInput, TemperatureMosRuntimeModel,
    TemperatureMosRuntimeModelContractVersionLiteral,
    TemperatureMosRuntimeModelMaterialCohortLiteral,
    TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
    TemperatureMosRuntimeModelMaterialScopeEnum as Scope,
    TemperatureMosRuntimeModelMaterialStrengthBands, TemperatureMosRuntimeRawReason as Reason,
    TemperatureMosRuntimeRecentErrorState, TemperatureMosRuntimeRecentErrorStateCohortLiteral,
    TemperatureMosRuntimeResult, TemperatureMosRuntimeStrengthBand, TemperatureMosServingModel,
};

const HOUR: i64 = 3_600_000;
const MONTH_STARTS: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

/// Evaluate only the source fit-only infer operation and retain every raw fallback in source order.
pub fn evaluate_native_temperature_fit_batch(value: &JsonValue) -> Option<JsonValue> {
    let JsonValue::Array(inputs) = field(value, "inputs")? else {
        return None;
    };
    if text(value, "operation")?.as_str() != "infer" || inputs.len() > 256 {
        return None;
    }
    let calendar = NativePredecessorModelCalendar::new()?;
    let admission = NativeApiModelAdmission::production().ok()?;
    let wire = NativeWirePrimitives;
    let mut results = Vec::with_capacity(inputs.len());
    // preserve original per-row outcomes without replacing missing model state by invented values
    for input in inputs {
        let raw = number(field(input, "forecast")?, "rawTemperatureC")?;
        let result = decode_and_evaluate(input, &calendar, &admission, &wire);
        results.push(match result {
            Ok(result) => result_value(result),
            Err(reason) => raw_value(raw, reason),
        });
    }
    Some(JsonValue::Array(results))
}

// admit the original source typed forecast, model and causal state
fn decode_and_evaluate(
    value: &JsonValue,
    calendar: &NativePredecessorModelCalendar,
    admission: &NativeApiModelAdmission,
    wire: &NativeWirePrimitives,
) -> Result<TemperatureMosRuntimeResult, Reason> {
    let model = field(value, "model").ok_or(Reason::InvalidModel)?;
    let scope = scope(model).ok_or(Reason::InvalidModel)?;
    let forecast = field(value, "forecast").ok_or(Reason::InvalidForecast)?;
    if text(forecast, "cohort").as_deref() != Some("ecmwf_single_run_hindcast") {
        return Err(Reason::UnsupportedCohort);
    }
    let lead = uint(forecast, "modelLeadHours").ok_or(match scope {
        Scope::InitializationFirst12 => Reason::OutsideInitializationFirst12,
        Scope::AssumedDelay6Next12 => Reason::OutsideAssumedDelay6Next12,
    })?;
    let forecast = TemperatureMosRuntimeForecast {
        cohort: TemperatureMosRuntimeForecastCohortLiteral,
        key: text(forecast, "key").ok_or(Reason::InvalidForecast)?,
        modelCycle: match text(forecast, "modelCycle").as_deref() {
            Some("49r1") => TemperatureMosRuntimeForecastModelCycleEnum::Value49r1,
            Some("50r1") => TemperatureMosRuntimeForecastModelCycleEnum::Value50r1,
            _ => return Err(Reason::InvalidForecast),
        },
        modelLeadHours: lead,
        rawTemperatureC: number(forecast, "rawTemperatureC").ok_or(Reason::InvalidForecast)?,
        rawRelativeHumidityPercent: nullable_number(forecast, "rawRelativeHumidityPercent")
            .ok_or(Reason::InvalidForecast)?,
        rawWindSpeedMps: nullable_number(forecast, "rawWindSpeedMps")
            .ok_or(Reason::InvalidForecast)?,
        runInitializedAt: text(forecast, "runInitializedAt").ok_or(Reason::InvalidForecast)?,
        validAt: text(forecast, "validAt").ok_or(Reason::InvalidForecast)?,
    };
    // retain exact supported source scope and monthly/permanent model discrimination
    if text(model, "cohort").as_deref() != Some("ecmwf_single_run_hindcast") {
        return Err(Reason::ModelIdentityMismatch);
    }
    let model = decode_model(model, scope).ok_or(Reason::InvalidModel)?;
    let state =
        decode_state(field(value, "recentErrorState").ok_or(Reason::InvalidRecentErrorState)?)
            .ok_or(Reason::InvalidRecentErrorState)?;
    let harmonics = calendar_harmonics(calendar, &forecast).ok_or(Reason::InvalidForecast)?;
    evaluate_temperature_fit_row(
        &TemperatureMosRuntimeInput {
            forecast,
            model,
            recentErrorState: state,
        },
        harmonics,
        wire,
        admission,
    )
}

// derive the original fixed harmonic basis from actual pinned local-calendar fields
fn calendar_harmonics(
    calendar: &NativePredecessorModelCalendar,
    forecast: &TemperatureMosRuntimeForecast,
) -> Option<AdmittedTemperatureMosCalendar> {
    let local = calendar.features(&forecast.validAt).ok()?;
    let run = parse_predecessor_source_utc(&forecast.runInitializedAt)?.epoch_milliseconds;
    let year: i16 = local.local_date.get(..4)?.parse().ok()?;
    let leap = year % 400 == 0 || (year % 4 == 0 && year % 100 != 0);
    let mut day = *MONTH_STARTS.get(usize::from(local.month.checked_sub(1)?))?
        + u16::from(local.local_day.checked_sub(1)?);
    if local.month > 2 && leap {
        day = day.checked_add(1)?;
    }
    let annual = std::f64::consts::TAU * (f64::from(day) + f64::from(local.hour) / 24.0)
        / if leap { 366.0 } else { 365.0 };
    let daily = std::f64::consts::TAU * f64::from(local.hour) / 24.0;
    let cycle = std::f64::consts::TAU * (run.rem_euclid(24 * HOUR) / HOUR) as f64 / 24.0;
    Some(AdmittedTemperatureMosCalendar {
        annual_basis: [
            annual.sin(),
            annual.cos(),
            (2.0 * annual).sin(),
            (2.0 * annual).cos(),
        ],
        daily_basis: [
            daily.sin(),
            daily.cos(),
            (2.0 * daily).sin(),
            (2.0 * daily).cos(),
        ],
        cycle_basis: [cycle.sin(), cycle.cos()],
    })
}

// select the exact two generated model scopes
fn scope(value: &JsonValue) -> Option<Scope> {
    match text(value, "scope")?.as_str() {
        "initialization_first12" => Some(Scope::InitializationFirst12),
        "assumed_delay6_next12" => Some(Scope::AssumedDelay6Next12),
        _ => None,
    }
}

// decode generated monthly/permanent fields preserving supported nullable vectors
fn decode_model(value: &JsonValue, scope: Scope) -> Option<TemperatureMosServingModel> {
    if text(value, "learnedStrengthContractVersion")?.as_str()
        != "temperature-winner-extensions-research/v1"
    {
        return None;
    }
    let direct = coefficients(value, "directCoefficients")?;
    let adaptive = coefficients(value, "adaptiveCoefficients")?;
    let supported = boolean(value, "supported")?;
    let cutoff = text(value, "trainingCutoffUtc")?;
    let bands = field(value, "strengthBands")?;
    let strength = TemperatureMosRuntimeModelMaterialStrengthBands {
        hours1To6: band(field(bands, "1-6")?)?,
        hours7To12: band(field(bands, "7-12")?)?,
    };
    match text(value, "contractVersion")?.as_str() {
        "temperature-shortlead-models-research/v1" => Some(TemperatureMosServingModel::Monthly(
            TemperatureMosRuntimeModel {
                adaptiveCoefficients: adaptive,
                directCoefficients: direct,
                cohort: TemperatureMosRuntimeModelMaterialCohortLiteral,
                learnedStrengthContractVersion:
                    TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
                scope,
                strengthBands: strength,
                supported,
                trainingCutoffUtc: cutoff,
                contractVersion: TemperatureMosRuntimeModelContractVersionLiteral,
                month: text(value, "month")?,
            },
        )),
        "temperature-permanent-model/v1" => Some(TemperatureMosServingModel::Permanent(
            TemperatureMosPermanentRuntimeModel {
                adaptiveCoefficients: adaptive,
                directCoefficients: direct,
                cohort: TemperatureMosRuntimeModelMaterialCohortLiteral,
                learnedStrengthContractVersion:
                    TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
                scope,
                strengthBands: strength,
                supported,
                trainingCutoffUtc: cutoff,
                contractVersion: TemperatureMosPermanentRuntimeModelContractVersionLiteral,
                effectiveFrom: text(value, "effectiveFrom")?,
                latestTrainingValidAt: text(value, "latestTrainingValidAt")?,
            },
        )),
        _ => None,
    }
}

// decode the complete generated recent-error chronology and selected source keys
fn decode_state(value: &JsonValue) -> Option<TemperatureMosRuntimeRecentErrorState> {
    if text(value, "cohort")?.as_str() != "ecmwf_single_run_hindcast" {
        return None;
    }
    let JsonValue::Array(keys) = field(value, "sourceKeys")? else {
        return None;
    };
    let keys = keys
        .iter()
        .map(|key| {
            let JsonValue::String(units) = key else {
                return None;
            };
            String::from_utf16(units).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    Some(TemperatureMosRuntimeRecentErrorState {
        b24C: nullable_number(value, "b24C")?,
        b72C: nullable_number(value, "b72C")?,
        cohort: TemperatureMosRuntimeRecentErrorStateCohortLiteral,
        localDates: uint(value, "localDates")?,
        mad72C: nullable_number(value, "mad72C")?,
        maximumSourceRunInitializedAt: nullable_text(value, "maximumSourceRunInitializedAt")?,
        maximumSourceValidAt: nullable_text(value, "maximumSourceValidAt")?,
        n24: uint(value, "n24")?,
        n72: uint(value, "n72")?,
        sourceKeys: keys,
        supported: boolean(value, "supported")?,
        targetRunInitializedAt: text(value, "targetRunInitializedAt")?,
        windowEndValidAt: text(value, "windowEndValidAt")?,
    })
}

// preserve the source exact band values prior to causal validation
fn band(value: &JsonValue) -> Option<TemperatureMosRuntimeStrengthBand> {
    Some(TemperatureMosRuntimeStrengthBand {
        alpha: number(value, "alpha")?,
        supported: boolean(value, "supported")?,
        trainingCutoffUtc: text(value, "trainingCutoffUtc")?,
    })
}

// admit one source nullable vector without forcing unsupported coefficients
fn coefficients(value: &JsonValue, key: &str) -> Option<Nullable<Vec<f64>>> {
    match field(value, key)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::Array(values) => Some(Nullable::Value(
            values
                .iter()
                .map(|value| match value {
                    JsonValue::Number(n) if n.is_finite() => Some(*n),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?,
        )),
        _ => None,
    }
}

// decode finite nullable weather/statistic fields
fn nullable_number(value: &JsonValue, key: &str) -> Option<Nullable<f64>> {
    if *field(value, key)? == JsonValue::Null {
        Some(Nullable::Null)
    } else {
        number(value, key).map(Nullable::Value)
    }
}

// decode the source nullable time labels
fn nullable_text(value: &JsonValue, key: &str) -> Option<Nullable<String>> {
    if *field(value, key)? == JsonValue::Null {
        Some(Nullable::Null)
    } else {
        text(value, key).map(Nullable::Value)
    }
}

// decode original unsigned JSON integers
fn uint(value: &JsonValue, key: &str) -> Option<u64> {
    let n = number(value, key)?;
    (n.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&n)).then_some(n as u64)
}

// admit source booleans without coercion
fn boolean(value: &JsonValue, key: &str) -> Option<bool> {
    match field(value, key)? {
        JsonValue::Boolean(v) => Some(*v),
        _ => None,
    }
}

// project original raw fallback result
fn raw_value(raw: f64, reason: Reason) -> JsonValue {
    let reason = Reason::WIRE_VALUES
        .iter()
        .find_map(|(name, value)| (*value == reason).then_some(*name))
        .unwrap_or("inference_error");
    object(vec![
        ("applied", JsonValue::Boolean(false)),
        ("predictionTemperatureC", JsonValue::Number(raw)),
        ("rawTemperatureC", JsonValue::Number(raw)),
        ("reason", string(reason)),
    ])
}

// project every generated applied numerical output and original branch identity
fn result_value(result: TemperatureMosRuntimeResult) -> JsonValue {
    match result {
        TemperatureMosRuntimeResult::Raw(result) => {
            raw_value(result.rawTemperatureC, result.reason)
        }
        TemperatureMosRuntimeResult::Applied(result) => {
            let branch = match result.branch {
                weather_contracts::TemperatureMosRuntimeAppliedResultBranchEnum::Adaptive => {
                    "adaptive"
                }
                weather_contracts::TemperatureMosRuntimeAppliedResultBranchEnum::Direct => "direct",
            };
            object(vec![
                ("alpha", JsonValue::Number(result.alpha)),
                ("applied", JsonValue::Boolean(true)),
                ("branch", string(branch)),
                (
                    "incumbentHalfStrengthTemperatureC",
                    JsonValue::Number(result.incumbentHalfStrengthTemperatureC),
                ),
                (
                    "predictionTemperatureC",
                    JsonValue::Number(result.predictionTemperatureC),
                ),
                ("rawTemperatureC", JsonValue::Number(result.rawTemperatureC)),
                ("reason", JsonValue::Null),
                (
                    "unscaledCorrectionC",
                    JsonValue::Number(result.unscaledCorrectionC),
                ),
            ])
        }
    }
}
