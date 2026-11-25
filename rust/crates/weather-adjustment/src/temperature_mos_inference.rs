use weather_contracts::{
    Nullable, TemperatureMosPermanentRuntimeModel, TemperatureMosRuntimeAppliedResult,
    TemperatureMosRuntimeAppliedResultAppliedLiteral, TemperatureMosRuntimeAppliedResultBranchEnum,
    TemperatureMosRuntimeAppliedResultReasonNull, TemperatureMosRuntimeForecastModelCycleEnum,
    TemperatureMosRuntimeModel, TemperatureMosRuntimeModelMaterialScopeEnum as TemperatureMosScope,
    TemperatureMosRuntimeModelMaterialStrengthBands, TemperatureMosRuntimeRawReason,
    TemperatureMosRuntimeRawResult, TemperatureMosRuntimeRawResultAppliedLiteral,
    TemperatureMosRuntimeRecentErrorState, TemperatureMosRuntimeResult, TemperatureMosServingModel,
};

const DIRECT_FEATURE_COUNT: usize = 35;
const ADAPTIVE_FEATURE_COUNT: usize = 49;
const CORRECTION_WEIGHT: f64 = 0.5;
const MAXIMUM_CORRECTION_C: f64 = 3.0;
const PHYSICAL_MINIMUM_C: f64 = -100.0;
const PHYSICAL_MAXIMUM_C: f64 = 70.0;
const DELAYED_OPERATIONAL_OFFSET_HOURS: u64 = 6;
const SHORT_DECAY_BITS: [u64; 12] = [
    0x3feb1660d7a223b0,
    0x3fe6edd3122f2ea5,
    0x3fe368b2fc6f960a,
    0x3fe06de9bcee72de,
    0x3fdbd075011c09aa,
    0x3fd78b56362cef38,
    0x3fd3ee07c9a54225,
    0x3fd0dec687e1adf1,
    0x3fcc8f87724b5c1d,
    0x3fc82d1364998cea,
    0x3fc476f0845a3fb8,
    0x3fc152aaa3bf81cc,
];
const LONG_DECAY_BITS: [u64; 12] = [
    0x3fee454ccac9798a,
    0x3feca286101c9102,
    0x3feb1660d7a223b0,
    0x3fe99fa40bc6c5f7,
    0x3fe83d27824a69c6,
    0x3fe6edd3122f2ea5,
    0x3fe5b09db64e5df6,
    0x3fe4848cbbe4955b,
    0x3fe368b2fc6f960a,
    0x3fe25c3022412203,
    0x3fe15e2ff732c652,
    0x3fe06de9bcee72de,
];

/// Already resolved source harmonics at the time/calendar boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdmittedTemperatureMosCalendar {
    pub annual_basis: [f64; 4],
    pub cycle_basis: [f64; 2],
    pub daily_basis: [f64; 4],
}

/// Weather, calendar, and causal state used by the source feature owners.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedTemperatureMosFeatureInput<'a> {
    pub calendar: AdmittedTemperatureMosCalendar,
    pub model_cycle: TemperatureMosRuntimeForecastModelCycleEnum,
    pub model_lead_hours: u64,
    pub raw_relative_humidity_percent: &'a Nullable<f64>,
    pub raw_temperature_c: f64,
    pub raw_wind_speed_mps: &'a Nullable<f64>,
    pub recent_error_state: &'a TemperatureMosRuntimeRecentErrorState,
    pub scope: TemperatureMosScope,
}

/// Owned direct and optional adaptive arrays in their frozen model order.
#[derive(Debug, Clone, PartialEq)]
pub struct TemperatureMosFeatureAssembly {
    pub direct: [f64; DIRECT_FEATURE_COUNT],
    pub adaptive: Option<[f64; ADAPTIVE_FEATURE_COUNT]>,
}

/// Borrowed monthly or permanent model material admitted for serving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorrowedTemperatureMosServingModel<'a> {
    Monthly(&'a TemperatureMosRuntimeModel),
    Permanent(&'a TemperatureMosPermanentRuntimeModel),
}

impl<'a> From<&'a TemperatureMosServingModel> for BorrowedTemperatureMosServingModel<'a> {
    /// Borrow one generated serving-model variant without cloning its material.
    fn from(model: &'a TemperatureMosServingModel) -> Self {
        // retain the generated monthly versus permanent selection
        match model {
            TemperatureMosServingModel::Monthly(model) => Self::Monthly(model),
            TemperatureMosServingModel::Permanent(model) => Self::Permanent(model),
        }
    }
}

impl BorrowedTemperatureMosServingModel<'_> {
    /// Return the selected model's generated lead-window scope.
    pub fn scope(self) -> TemperatureMosScope {
        // expose the shared field without erasing the selected model generation
        match self {
            Self::Monthly(model) => model.scope,
            Self::Permanent(model) => model.scope,
        }
    }
}

/// Full admitted application input composed through the feature owners.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedTemperatureMosApplicationInput<'a, Model = &'a TemperatureMosServingModel> {
    pub calendar: AdmittedTemperatureMosCalendar,
    pub model: Model,
    pub model_cycle: TemperatureMosRuntimeForecastModelCycleEnum,
    pub model_lead_hours: u64,
    pub raw_relative_humidity_percent: &'a Nullable<f64>,
    pub raw_temperature_c: f64,
    pub raw_wind_speed_mps: &'a Nullable<f64>,
    pub recent_error_state: &'a TemperatureMosRuntimeRecentErrorState,
}

/// Caller-normalized features after calendar and timestamp admission.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedTemperatureMosFeatures<'a> {
    pub direct: &'a [f64; DIRECT_FEATURE_COUNT],
    pub adaptive: Option<&'a [f64; ADAPTIVE_FEATURE_COUNT]>,
}

/// Numerical runtime inputs after identity, cutoff, and causal-state admission.
#[derive(Debug, Clone, Copy)]
pub struct AdmittedTemperatureMosRuntimeInput<'a, Model = &'a TemperatureMosServingModel> {
    pub features: AdmittedTemperatureMosFeatures<'a>,
    pub model: Model,
    pub model_lead_hours: u64,
    pub raw_temperature_c: f64,
    pub recent_error_state: &'a TemperatureMosRuntimeRecentErrorState,
}

struct ModelMaterial<'a> {
    adaptive_coefficients: &'a Nullable<Vec<f64>>,
    direct_coefficients: &'a Nullable<Vec<f64>>,
    scope: TemperatureMosScope,
    strength_bands: &'a TemperatureMosRuntimeModelMaterialStrengthBands,
    supported: bool,
}

/// Build the source-ordered 35/49 temperature MOS feature arrays.
pub fn build_admitted_temperature_mos_features(
    input: AdmittedTemperatureMosFeatureInput<'_>,
) -> Result<TemperatureMosFeatureAssembly, TemperatureMosRuntimeRawReason> {
    // retain the forecast lead rejection before feature arithmetic
    if !lead_is_supported(input.scope, input.model_lead_hours) {
        return Err(match input.scope {
            TemperatureMosScope::InitializationFirst12 => {
                TemperatureMosRuntimeRawReason::OutsideInitializationFirst12
            }
            TemperatureMosScope::AssumedDelay6Next12 => {
                TemperatureMosRuntimeRawReason::OutsideAssumedDelay6Next12
            }
        });
    }
    // reject unresolved source harmonics before weather normalization
    if input
        .calendar
        .annual_basis
        .iter()
        .chain(input.calendar.daily_basis.iter())
        .chain(input.calendar.cycle_basis.iter())
        .any(|value| !value.is_finite())
    {
        return Err(TemperatureMosRuntimeRawReason::InvalidForecast);
    }
    // require finite provider weather before normalization
    if !input.raw_temperature_c.is_finite()
        || nullable_nonfinite(input.raw_relative_humidity_percent)
        || nullable_nonfinite(input.raw_wind_speed_mps)
    {
        return Err(TemperatureMosRuntimeRawReason::InvalidForecast);
    }

    let annual_basis = input.calendar.annual_basis;
    let daily_basis = input.calendar.daily_basis;
    let temperature = (input.raw_temperature_c - 10.0) / 10.0;
    let humidity_missing = matches!(input.raw_relative_humidity_percent, Nullable::Null);
    let wind_missing = matches!(input.raw_wind_speed_mps, Nullable::Null);
    let humidity = nullable_or(input.raw_relative_humidity_percent, 75.0, 25.0);
    let wind = nullable_or(input.raw_wind_speed_mps, 2.0, 3.0);
    let lead_scaled = input.model_lead_hours as f64 / 18.0;
    let direct = [
        1.0,
        annual_basis[0],
        annual_basis[1],
        annual_basis[2],
        annual_basis[3],
        daily_basis[0],
        daily_basis[1],
        daily_basis[2],
        daily_basis[3],
        annual_basis[0] * daily_basis[0],
        annual_basis[0] * daily_basis[1],
        annual_basis[1] * daily_basis[0],
        annual_basis[1] * daily_basis[1],
        temperature,
        temperature * temperature,
        humidity,
        wind,
        boolean_feature(humidity_missing),
        boolean_feature(wind_missing),
        temperature * annual_basis[0],
        temperature * annual_basis[1],
        temperature * daily_basis[0],
        temperature * daily_basis[1],
        humidity * daily_basis[0],
        humidity * daily_basis[1],
        wind * daily_basis[0],
        wind * daily_basis[1],
        lead_scaled,
        lead_scaled * lead_scaled,
        lead_scaled * daily_basis[0],
        lead_scaled * daily_basis[1],
        lead_scaled * temperature,
        input.calendar.cycle_basis[0],
        input.calendar.cycle_basis[1],
        boolean_feature(matches!(
            input.model_cycle,
            TemperatureMosRuntimeForecastModelCycleEnum::Value50r1
        )),
    ];
    // preserve the static feature builder's finite result guard
    if direct.iter().any(|value| !value.is_finite()) {
        return Err(TemperatureMosRuntimeRawReason::InvalidForecast);
    }

    let adaptive = if input.recent_error_state.supported {
        Some(build_adaptive_features(&direct, &input)?)
    } else {
        None
    };
    Ok(TemperatureMosFeatureAssembly { adaptive, direct })
}

/// Compose the feature owner directly into the numerical evaluator.
pub fn apply_admitted_temperature_mos_forecast<'a, Model>(
    input: AdmittedTemperatureMosApplicationInput<'a, Model>,
) -> TemperatureMosRuntimeResult
where
    Model: Into<BorrowedTemperatureMosServingModel<'a>>,
{
    let selected_model = input.model.into();
    let model = model_material(selected_model);
    let features =
        match build_admitted_temperature_mos_features(AdmittedTemperatureMosFeatureInput {
            calendar: input.calendar,
            model_cycle: input.model_cycle,
            model_lead_hours: input.model_lead_hours,
            raw_relative_humidity_percent: input.raw_relative_humidity_percent,
            raw_temperature_c: input.raw_temperature_c,
            raw_wind_speed_mps: input.raw_wind_speed_mps,
            recent_error_state: input.recent_error_state,
            scope: model.scope,
        }) {
            Ok(features) => features,
            Err(reason) => return raw_result(input.raw_temperature_c, reason),
        };
    apply_admitted_temperature_mos_runtime(AdmittedTemperatureMosRuntimeInput {
        features: AdmittedTemperatureMosFeatures {
            direct: &features.direct,
            adaptive: features.adaptive.as_ref(),
        },
        model: selected_model,
        model_lead_hours: input.model_lead_hours,
        raw_temperature_c: input.raw_temperature_c,
        recent_error_state: input.recent_error_state,
    })
}

/// Append the exact causal recent-error terms to one direct vector.
fn build_adaptive_features(
    direct: &[f64; DIRECT_FEATURE_COUNT],
    input: &AdmittedTemperatureMosFeatureInput<'_>,
) -> Result<[f64; ADAPTIVE_FEATURE_COUNT], TemperatureMosRuntimeRawReason> {
    let short_value = nullable_value(&input.recent_error_state.b24C);
    let long_value = nullable_value(&input.recent_error_state.b72C);
    let difference = match (
        &input.recent_error_state.b24C,
        &input.recent_error_state.b72C,
    ) {
        (Nullable::Value(short), Nullable::Value(long)) => short - long,
        _ => 0.0,
    };
    let signed = [short_value / 3.0, long_value / 3.0, difference / 3.0];
    let operational_horizon_hours = match input.scope {
        TemperatureMosScope::InitializationFirst12 => input.model_lead_hours,
        TemperatureMosScope::AssumedDelay6Next12 => {
            input.model_lead_hours - DELAYED_OPERATIONAL_OFFSET_HOURS
        }
    };
    let decay_index = usize::try_from(operational_horizon_hours - 1)
        .expect("validated operational horizon fits usize");
    let decay_short = f64::from_bits(SHORT_DECAY_BITS[decay_index]);
    let decay_long = f64::from_bits(LONG_DECAY_BITS[decay_index]);
    let mut adaptive = [0.0; ADAPTIVE_FEATURE_COUNT];
    adaptive[..DIRECT_FEATURE_COUNT].copy_from_slice(direct);
    adaptive[35] = signed[0];
    adaptive[36] = signed[1];
    adaptive[37] = signed[2];
    adaptive[38] = nullable_value(&input.recent_error_state.mad72C) / 3.0;
    adaptive[39] = input.recent_error_state.n24 as f64 / 24.0;
    adaptive[40] = input.recent_error_state.n72 as f64 / 72.0;
    adaptive[41] = boolean_feature(matches!(&input.recent_error_state.b24C, Nullable::Null));
    adaptive[42] = boolean_feature(matches!(&input.recent_error_state.b72C, Nullable::Null));
    adaptive[43] = signed[0] * decay_short;
    adaptive[44] = signed[1] * decay_short;
    adaptive[45] = signed[2] * decay_short;
    adaptive[46] = signed[0] * decay_long;
    adaptive[47] = signed[1] * decay_long;
    adaptive[48] = signed[2] * decay_long;
    // retain the adaptive feature builder's finite state guard
    if adaptive.iter().any(|value| !value.is_finite()) {
        return Err(TemperatureMosRuntimeRawReason::InvalidRecentErrorState);
    }
    Ok(adaptive)
}

/// Report whether one optional generated weather value is nonfinite.
fn nullable_nonfinite(value: &Nullable<f64>) -> bool {
    // accept absence while rejecting undefined present values
    match value {
        Nullable::Null => false,
        Nullable::Value(value) => !value.is_finite(),
    }
}

/// Normalize one optional weather value or use its exact zero imputation.
fn nullable_or(value: &Nullable<f64>, center: f64, scale: f64) -> f64 {
    // preserve source null imputation before centering
    match value {
        Nullable::Null => 0.0,
        Nullable::Value(value) => (value - center) / scale,
    }
}

/// Read one optional recent-error statistic with zero imputation.
fn nullable_value(value: &Nullable<f64>) -> f64 {
    // preserve absent causal-statistic imputation
    match value {
        Nullable::Null => 0.0,
        Nullable::Value(value) => *value,
    }
}

/// Encode one boolean indicator as the source numeric feature.
fn boolean_feature(value: bool) -> f64 {
    // preserve JavaScript Number boolean conversion
    if value { 1.0 } else { 0.0 }
}

/// Apply the admitted temperature MOS model without time, serialization, or artifact I/O.
pub fn apply_admitted_temperature_mos_runtime<'a, Model>(
    input: AdmittedTemperatureMosRuntimeInput<'a, Model>,
) -> TemperatureMosRuntimeResult
where
    Model: Into<BorrowedTemperatureMosServingModel<'a>>,
{
    // reject nonfinite provider values before model selection
    if !input.raw_temperature_c.is_finite() {
        return raw_result(
            input.raw_temperature_c,
            TemperatureMosRuntimeRawReason::InvalidForecast,
        );
    }

    let model = model_material(input.model.into());
    // retain the source scope-specific lead windows
    if !lead_is_supported(model.scope, input.model_lead_hours) {
        let reason = match model.scope {
            TemperatureMosScope::InitializationFirst12 => {
                TemperatureMosRuntimeRawReason::OutsideInitializationFirst12
            }
            TemperatureMosScope::AssumedDelay6Next12 => {
                TemperatureMosRuntimeRawReason::OutsideAssumedDelay6Next12
            }
        };
        return raw_result(input.raw_temperature_c, reason);
    }

    // preserve the original model cold-start fallback before table validation
    if !model.supported {
        return raw_result(
            input.raw_temperature_c,
            TemperatureMosRuntimeRawReason::ModelNotSupported,
        );
    }

    let Some(direct_coefficients) =
        validated_coefficients(model.direct_coefficients, DIRECT_FEATURE_COUNT)
    else {
        return raw_result(
            input.raw_temperature_c,
            TemperatureMosRuntimeRawReason::InvalidModel,
        );
    };
    let Some(adaptive_coefficients) =
        validated_coefficients(model.adaptive_coefficients, ADAPTIVE_FEATURE_COUNT)
    else {
        return raw_result(
            input.raw_temperature_c,
            TemperatureMosRuntimeRawReason::InvalidModel,
        );
    };
    // validate both learned-strength bands before selecting a branch
    if !alpha_is_valid(model.strength_bands.hours1To6.alpha)
        || !alpha_is_valid(model.strength_bands.hours7To12.alpha)
    {
        return raw_result(
            input.raw_temperature_c,
            TemperatureMosRuntimeRawReason::InvalidModel,
        );
    }

    let (branch, features, coefficients) = if input.recent_error_state.supported {
        let Some(features) = input.features.adaptive else {
            return raw_result(
                input.raw_temperature_c,
                TemperatureMosRuntimeRawReason::InvalidRecentErrorState,
            );
        };
        // retain the adaptive feature builder's nonfinite-state fallback
        if features.iter().any(|feature| !feature.is_finite()) {
            return raw_result(
                input.raw_temperature_c,
                TemperatureMosRuntimeRawReason::InvalidRecentErrorState,
            );
        }
        (
            TemperatureMosRuntimeAppliedResultBranchEnum::Adaptive,
            features.as_slice(),
            adaptive_coefficients,
        )
    } else {
        // retain the static feature builder's nonfinite-forecast fallback
        if input
            .features
            .direct
            .iter()
            .any(|feature| !feature.is_finite())
        {
            return raw_result(
                input.raw_temperature_c,
                TemperatureMosRuntimeRawReason::InvalidForecast,
            );
        }
        (
            TemperatureMosRuntimeAppliedResultBranchEnum::Direct,
            input.features.direct.as_slice(),
            direct_coefficients,
        )
    };

    let Some(unscaled_correction_c) = dot(features, coefficients) else {
        return raw_result(
            input.raw_temperature_c,
            TemperatureMosRuntimeRawReason::InvalidModel,
        );
    };
    let operational_horizon_hours = match model.scope {
        TemperatureMosScope::InitializationFirst12 => input.model_lead_hours,
        TemperatureMosScope::AssumedDelay6Next12 => {
            input.model_lead_hours - DELAYED_OPERATIONAL_OFFSET_HOURS
        }
    };
    let band = if operational_horizon_hours <= 6 {
        &model.strength_bands.hours1To6
    } else {
        &model.strength_bands.hours7To12
    };

    // fail raw instead of substituting the incumbent half-strength result
    if !band.supported {
        return raw_result(
            input.raw_temperature_c,
            TemperatureMosRuntimeRawReason::StrengthBandNotSupported,
        );
    }

    TemperatureMosRuntimeResult::Applied(TemperatureMosRuntimeAppliedResult {
        alpha: band.alpha,
        applied: TemperatureMosRuntimeAppliedResultAppliedLiteral,
        branch,
        incumbentHalfStrengthTemperatureC: adjusted_temperature(
            input.raw_temperature_c,
            unscaled_correction_c,
            CORRECTION_WEIGHT,
        ),
        predictionTemperatureC: adjusted_temperature(
            input.raw_temperature_c,
            unscaled_correction_c,
            band.alpha,
        ),
        rawTemperatureC: input.raw_temperature_c,
        reason: TemperatureMosRuntimeAppliedResultReasonNull,
        unscaledCorrectionC: unscaled_correction_c,
    })
}

/// Borrow the shared numerical fields from either generated model contract.
fn model_material(model: BorrowedTemperatureMosServingModel<'_>) -> ModelMaterial<'_> {
    // erase only the monthly versus permanent identity already admitted by the caller
    match model {
        BorrowedTemperatureMosServingModel::Monthly(model) => ModelMaterial {
            adaptive_coefficients: &model.adaptiveCoefficients,
            direct_coefficients: &model.directCoefficients,
            scope: model.scope,
            strength_bands: &model.strengthBands,
            supported: model.supported,
        },
        BorrowedTemperatureMosServingModel::Permanent(model) => ModelMaterial {
            adaptive_coefficients: &model.adaptiveCoefficients,
            direct_coefficients: &model.directCoefficients,
            scope: model.scope,
            strength_bands: &model.strengthBands,
            supported: model.supported,
        },
    }
}

/// Check the exact initialization-relative or delayed model lead window.
fn lead_is_supported(scope: TemperatureMosScope, model_lead_hours: u64) -> bool {
    // preserve inclusive source boundaries for both fitted scopes
    match scope {
        TemperatureMosScope::InitializationFirst12 => (1..=12).contains(&model_lead_hours),
        TemperatureMosScope::AssumedDelay6Next12 => (7..=18).contains(&model_lead_hours),
    }
}

/// Validate one generated coefficient vector without copying it.
fn validated_coefficients(coefficients: &Nullable<Vec<f64>>, width: usize) -> Option<&[f64]> {
    let Nullable::Value(coefficients) = coefficients else {
        return None;
    };
    // require exact width and finite material before inference
    if coefficients.len() != width || coefficients.iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some(coefficients)
}

/// Check membership in the frozen learned-strength grid.
fn alpha_is_valid(alpha: f64) -> bool {
    alpha == 0.35 || alpha == 0.5 || alpha == 0.65 || alpha == 0.8 || alpha == 1.0
}

/// Accumulate one coefficient dot product in source feature order.
fn dot(features: &[f64], coefficients: &[f64]) -> Option<f64> {
    let mut result = 0.0;
    // retain JavaScript's left-to-right accumulation order
    for index in 0..features.len() {
        result += features[index] * coefficients[index];
    }
    // contain multiplication or accumulation overflow as invalid model material
    if !result.is_finite() {
        return None;
    }
    Some(result)
}

/// Apply the exact correction and physical output caps.
fn adjusted_temperature(raw: f64, unscaled: f64, alpha: f64) -> f64 {
    let correction = (alpha * unscaled).clamp(-MAXIMUM_CORRECTION_C, MAXIMUM_CORRECTION_C);
    (raw + correction).clamp(PHYSICAL_MINIMUM_C, PHYSICAL_MAXIMUM_C)
}

/// Construct the generated raw-preserving numerical result.
fn raw_result(
    raw_temperature_c: f64,
    reason: TemperatureMosRuntimeRawReason,
) -> TemperatureMosRuntimeResult {
    TemperatureMosRuntimeResult::Raw(TemperatureMosRuntimeRawResult {
        applied: TemperatureMosRuntimeRawResultAppliedLiteral,
        predictionTemperatureC: raw_temperature_c,
        rawTemperatureC: raw_temperature_c,
        reason,
    })
}
