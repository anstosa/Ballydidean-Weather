//! Validate temperature MOS model material against one source run and valid hour.

use weather_adjustment::BorrowedTemperatureMosServingModel;
use weather_contracts::{
    JsDateOrString, Nullable, TemperatureMosRuntimeForecastCohortLiteral,
    TemperatureMosRuntimeModel, TemperatureMosRuntimeModelMaterialCohortLiteral,
    TemperatureMosRuntimeModelMaterialStrengthBands, TemperatureMosRuntimeRawReason,
    TemperatureMosRuntimeStrengthBand,
};

use crate::maintenance_source_projection::{admit_source_clock, admit_source_wire_clock};
use crate::{CanonicalInstant, CurrentTimeResolver, ForecastTemperatureMosAdmissionBoundary};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const MONTHLY_EMBARGO_HOURS: i64 = 168;
const PERMANENT_TRAINING_LAG_HOURS: i64 = 7;
const DIRECT_COEFFICIENTS: usize = 35;
const ADAPTIVE_COEFFICIENTS: usize = 49;
const ALPHA_GRID: [f64; 5] = [0.35, 0.5, 0.65, 0.8, 1.0];

/// Apply the original model and strength-band causal gates in Node order.
pub(crate) fn validate_mos_model_causality(
    model: BorrowedTemperatureMosServingModel<'_>,
    run: &CanonicalInstant,
    valid: &CanonicalInstant,
    resolver: &dyn CurrentTimeResolver,
    admission: &dyn ForecastTemperatureMosAdmissionBoundary,
) -> Result<(), TemperatureMosRuntimeRawReason> {
    let material = model_material(model);
    // generated cohort literals still remain bound across model and forecast contracts
    if TemperatureMosRuntimeModelMaterialCohortLiteral::WIRE_VALUE
        != TemperatureMosRuntimeForecastCohortLiteral::WIRE_VALUE
    {
        return Err(TemperatureMosRuntimeRawReason::ModelIdentityMismatch);
    }
    let cutoff = model_instant(material.training_cutoff_utc, resolver)?;
    // same-run or future fitting material is never available to this forecast
    if cutoff.epoch_milliseconds >= run.epoch_milliseconds {
        return Err(TemperatureMosRuntimeRawReason::ModelNotYetAvailable);
    }

    // preserve monthly embargo and permanent serving-epoch ordering before support checks
    match model {
        BorrowedTemperatureMosServingModel::Monthly(monthly) => {
            validate_monthly_causality(monthly, &cutoff, valid, resolver, admission)?;
        }
        BorrowedTemperatureMosServingModel::Permanent(permanent) => {
            let effective_from = model_instant(&permanent.effectiveFrom, resolver)?;
            let latest_training = model_instant(&permanent.latestTrainingValidAt, resolver)?;
            let latest_training_with_lag = checked_hours_after(
                latest_training.epoch_milliseconds,
                PERMANENT_TRAINING_LAG_HOURS,
            )?;
            // prohibit future training labels and inconsistent permanent cutoffs
            if latest_training_with_lag > cutoff.epoch_milliseconds
                || cutoff.epoch_milliseconds > effective_from.epoch_milliseconds
            {
                return Err(TemperatureMosRuntimeRawReason::InvalidModel);
            }
            // retain the permanent model's declared first serving instant
            if valid.epoch_milliseconds < effective_from.epoch_milliseconds {
                return Err(TemperatureMosRuntimeRawReason::ModelNotYetAvailable);
            }
        }
    }

    // unsupported winners fail before coefficient or strength-band shape inspection
    if !material.supported {
        return Err(TemperatureMosRuntimeRawReason::ModelNotSupported);
    }
    validate_coefficients(material.direct_coefficients, DIRECT_COEFFICIENTS)?;
    validate_coefficients(material.adaptive_coefficients, ADAPTIVE_COEFFICIENTS)?;
    let first_cutoff = validate_strength_band(&material.strength_bands.hours1To6, run, resolver)?;
    let second_cutoff = validate_strength_band(&material.strength_bands.hours7To12, run, resolver)?;
    // both learned alpha states must come from the winner's original fit
    if first_cutoff.epoch_milliseconds != cutoff.epoch_milliseconds
        || second_cutoff.epoch_milliseconds != cutoff.epoch_milliseconds
    {
        return Err(TemperatureMosRuntimeRawReason::InvalidModel);
    }
    Ok(())
}

struct ModelMaterial<'a> {
    direct_coefficients: &'a Nullable<Vec<f64>>,
    adaptive_coefficients: &'a Nullable<Vec<f64>>,
    strength_bands: &'a TemperatureMosRuntimeModelMaterialStrengthBands,
    supported: bool,
    training_cutoff_utc: &'a str,
}

/// Borrow the fields common to both generated serving-model generations.
fn model_material(model: BorrowedTemperatureMosServingModel<'_>) -> ModelMaterial<'_> {
    // retain the generated contract discriminator while sharing its closed material
    match model {
        BorrowedTemperatureMosServingModel::Monthly(model) => ModelMaterial {
            direct_coefficients: &model.directCoefficients,
            adaptive_coefficients: &model.adaptiveCoefficients,
            strength_bands: &model.strengthBands,
            supported: model.supported,
            training_cutoff_utc: &model.trainingCutoffUtc,
        },
        BorrowedTemperatureMosServingModel::Permanent(model) => ModelMaterial {
            direct_coefficients: &model.directCoefficients,
            adaptive_coefficients: &model.adaptiveCoefficients,
            strength_bands: &model.strengthBands,
            supported: model.supported,
            training_cutoff_utc: &model.trainingCutoffUtc,
        },
    }
}

/// Validate the fitted local month and its exact seven-day embargo.
fn validate_monthly_causality(
    model: &TemperatureMosRuntimeModel,
    cutoff: &CanonicalInstant,
    valid: &CanonicalInstant,
    resolver: &dyn CurrentTimeResolver,
    admission: &dyn ForecastTemperatureMosAdmissionBoundary,
) -> Result<(), TemperatureMosRuntimeRawReason> {
    // reject malformed month identities before consulting the local calendar
    if !valid_month(&model.month) {
        return Err(TemperatureMosRuntimeRawReason::ModelIdentityMismatch);
    }
    let (valid_local_date, _) = admission
        .local_date_hour(valid)
        .map_err(|_| TemperatureMosRuntimeRawReason::InferenceError)?;
    // serving remains confined to the fitted local calendar month
    if valid_local_date.get(..7) != Some(model.month.as_str()) {
        return Err(TemperatureMosRuntimeRawReason::ModelIdentityMismatch);
    }

    let embargo_epoch = checked_hours_after(cutoff.epoch_milliseconds, MONTHLY_EMBARGO_HOURS)?;
    let embargo = admit_source_clock(
        resolver,
        &JsDateOrString::DateEpochMilliseconds(embargo_epoch),
    )
    .map_err(|_| TemperatureMosRuntimeRawReason::InferenceError)?;
    let (embargo_local_date, embargo_local_hour) = admission
        .local_date_hour(&embargo)
        .map_err(|_| TemperatureMosRuntimeRawReason::InferenceError)?;
    let expected_local_date = format!("{}-01", model.month);
    // require an exact UTC hour whose embargo lands on local midnight day one
    if cutoff.epoch_milliseconds.rem_euclid(HOUR_MILLISECONDS) != 0
        || embargo_local_date != expected_local_date
        || embargo_local_hour != 0
    {
        return Err(TemperatureMosRuntimeRawReason::InvalidModel);
    }
    Ok(())
}

/// Validate one learned-strength cutoff and frozen-grid alpha.
fn validate_strength_band(
    band: &TemperatureMosRuntimeStrengthBand,
    run: &CanonicalInstant,
    resolver: &dyn CurrentTimeResolver,
) -> Result<CanonicalInstant, TemperatureMosRuntimeRawReason> {
    // generated support is already boolean, while alpha remains a runtime number
    if !ALPHA_GRID.contains(&band.alpha) {
        return Err(TemperatureMosRuntimeRawReason::InvalidModel);
    }
    let cutoff = model_instant(&band.trainingCutoffUtc, resolver)?;
    // strength learned from this run or a later run is not yet available
    if cutoff.epoch_milliseconds >= run.epoch_milliseconds {
        return Err(TemperatureMosRuntimeRawReason::ModelNotYetAvailable);
    }
    Ok(cutoff)
}

/// Require one exact coefficient width containing only finite values.
fn validate_coefficients(
    coefficients: &Nullable<Vec<f64>>,
    width: usize,
) -> Result<(), TemperatureMosRuntimeRawReason> {
    let Nullable::Value(coefficients) = coefficients else {
        return Err(TemperatureMosRuntimeRawReason::InvalidModel);
    };
    // preserve shape rejection before inspecting individual fitted values
    if coefficients.len() != width {
        return Err(TemperatureMosRuntimeRawReason::InvalidModel);
    }
    // no nonfinite coefficient may enter dot-product inference
    if coefficients
        .iter()
        .any(|coefficient| !coefficient.is_finite())
    {
        return Err(TemperatureMosRuntimeRawReason::InvalidModel);
    }
    Ok(())
}

/// Admit one model-owned canonical UTC instant with model-error semantics.
fn model_instant(
    value: &str,
    resolver: &dyn CurrentTimeResolver,
) -> Result<CanonicalInstant, TemperatureMosRuntimeRawReason> {
    admit_source_wire_clock(resolver, value)
        .map_err(|_| TemperatureMosRuntimeRawReason::InvalidModel)
}

/// Add one bounded whole-hour causal offset without wrapping.
fn checked_hours_after(
    epoch_milliseconds: i64,
    hours: i64,
) -> Result<i64, TemperatureMosRuntimeRawReason> {
    hours
        .checked_mul(HOUR_MILLISECONDS)
        .and_then(|offset| epoch_milliseconds.checked_add(offset))
        .ok_or(TemperatureMosRuntimeRawReason::InvalidModel)
}

/// Match JavaScript's exact ASCII `YYYY-MM` month grammar.
fn valid_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..].iter().all(u8::is_ascii_digit)
}
