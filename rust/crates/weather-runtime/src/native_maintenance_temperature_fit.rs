//! Pure native implementation of the reviewed monthly temperature fit-only command.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::CString;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::sync::OnceLock;

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::maintenance_canonical_json::{exact_keys, field, text};
use crate::native_maintenance_policy_comparison::{number, object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    NativePredecessorModelCalendar, NativeWirePrimitives, format_predecessor_source_utc,
    parse_predecessor_source_utc,
};

const INPUT_VERSION: &str = "temperature-maintenance-fit-input/v2";
const OUTPUT_VERSION: &str = "temperature-maintenance-fit/v2";
const MONTHLY_MODEL_VERSION: &str = "temperature-shortlead-models-research/v1";
const PERMANENT_MODEL_VERSION: &str = "temperature-permanent-model/v1";
const COHORT: &str = "ecmwf_single_run_hindcast";
const SCOPE: &str = "assumed_delay6_next12";
const EXPANDING: &str = "month_start_expanding";
const TRAILING: &str = "month_start_trailing365";
const MAXIMUM_ROWS: usize = 250_000;
const MAXIMUM_INPUT_BYTES: usize = 512 * 1024 * 1024;
const MAXIMUM_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;
const EMBARGO: i64 = 168 * HOUR;
const OPENBLAS_SHA256: &str = "432e1c1739731468ba305771af96773939e43016643e2d630f58cb45b6670e96";
const SANDBOX_OPENBLAS: &str = "/runtime/numpy.libs/libscipy_openblas64_-f48b354e.so";
#[cfg(test)]
const HOST_OPENBLAS: &str = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14/site-packages/numpy.libs/libscipy_openblas64_-f48b354e.so";
const CBLAS_ROW_MAJOR: i32 = 101;
const CBLAS_NO_TRANSPOSE: i32 = 111;
const CBLAS_TRANSPOSE: i32 = 112;

type Dgemm = unsafe extern "C" fn(
    i32,
    i32,
    i32,
    i64,
    i64,
    i64,
    f64,
    *const f64,
    i64,
    *const f64,
    i64,
    f64,
    *mut f64,
    i64,
);
type Dgemv = unsafe extern "C" fn(
    i32,
    i32,
    i64,
    i64,
    f64,
    *const f64,
    i64,
    *const f64,
    i64,
    f64,
    *mut f64,
    i64,
);
type Dgesv = unsafe extern "C" fn(
    *const i64,
    *const i64,
    *mut f64,
    *const i64,
    *mut i64,
    *mut f64,
    *const i64,
    *mut i64,
);
type SetThreads = unsafe extern "C" fn(i32);

#[derive(Clone, Copy)]
struct OpenBlas {
    dgemm: Dgemm,
    dgemv: Dgemv,
    dgesv: Dgesv,
}

static OPENBLAS: OnceLock<Option<OpenBlas>> = OnceLock::new();

/// Closed fit-only refusal without including private row material in diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceTemperatureFitError {
    Input,
    RowCeiling,
    Calendar,
    Provenance,
    Forecast,
    State,
    Incumbent,
    NumericalRuntime,
    Numeric,
    Codec,
    OutputCeiling,
}

impl Display for NativeMaintenanceTemperatureFitError {
    /// Render one bounded family-local failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "temperature fit input is invalid",
            Self::RowCeiling => "temperature fit row ceiling exceeded",
            Self::Calendar => "temperature fit calendar is invalid",
            Self::Provenance => "temperature fit provenance is invalid",
            Self::Forecast => "temperature fit forecast is invalid",
            Self::State => "temperature fit recent-error state is invalid",
            Self::Incumbent => "temperature fit incumbent is invalid",
            Self::NumericalRuntime => "temperature fit numerical runtime is unavailable",
            Self::Numeric => "temperature fit numerical result is invalid",
            Self::Codec => "temperature fit json codec failed",
            Self::OutputCeiling => "temperature fit candidate ceiling exceeded",
        })
    }
}

impl std::error::Error for NativeMaintenanceTemperatureFitError {}

/// Decode one bounded fit-only document and return the original canonical candidate framing.
pub fn run_native_temperature_fit_bytes(
    bytes: &[u8],
) -> Result<Vec<u8>, NativeMaintenanceTemperatureFitError> {
    // bound the retained private document before allocating its recursive value
    if bytes.len() > MAXIMUM_INPUT_BYTES {
        return Err(NativeMaintenanceTemperatureFitError::Input);
    }
    let value = NativeWirePrimitives
        .decode_json(bytes)
        .ok_or(NativeMaintenanceTemperatureFitError::Codec)?;
    run_native_temperature_fit_document(value)
}

/// Fit both frozen arms and return compact recursively sorted LF-terminated candidate bytes.
pub fn run_native_temperature_fit_document(
    value: JsonValue,
) -> Result<Vec<u8>, NativeMaintenanceTemperatureFitError> {
    let output = fit_document(&value)?;
    let mut bytes = encode_python_canonical(&output)?;
    bytes.push(b'\n');
    // retain the original independent public candidate ceiling
    if bytes.len() > MAXIMUM_OUTPUT_BYTES {
        return Err(NativeMaintenanceTemperatureFitError::OutputCeiling);
    }
    Ok(bytes)
}

struct FitInput<'a> {
    month: String,
    month_start: i64,
    incumbent: &'a JsonValue,
    training: Vec<TemperatureRow<'a>>,
    development: Vec<TemperatureRow<'a>>,
}

struct TemperatureRow<'a> {
    actual: f64,
    cohort: String,
    key: String,
    local_date: String,
    local_day_ordinal: i64,
    local_hour: u8,
    local_month: String,
    local_year: i64,
    model_cycle: String,
    model_lead: usize,
    operational_horizon: usize,
    raw: f64,
    humidity: Option<f64>,
    wind: Option<f64>,
    run_at: i64,
    run_text: String,
    source_receipt: i64,
    target_receipt: i64,
    valid_at: i64,
    valid_text: String,
    state: &'a JsonValue,
}

struct FittedModel {
    value: JsonValue,
    supported: bool,
    training_rows: usize,
    training_dates: usize,
    last_training_valid_at: Option<String>,
    training_keys: Vec<String>,
}

struct CandidateFit {
    model: JsonValue,
    receipt: JsonValue,
    supported: bool,
}

struct InferenceResult {
    applied: bool,
    prediction: f64,
}

/// Validate the closed source document before consulting any numerical label.
fn fit_document(value: &JsonValue) -> Result<JsonValue, NativeMaintenanceTemperatureFitError> {
    const KEYS: [&str; 5] = [
        "contractVersion",
        "month",
        "trainingRows",
        "developmentRows",
        "incumbentModel",
    ];
    // reject caller-defined policy or confirmation extensions
    if !exact_keys(value, &KEYS) || text(value, "contractVersion").as_deref() != Some(INPUT_VERSION)
    {
        return Err(NativeMaintenanceTemperatureFitError::Input);
    }
    let month = text(value, "month").ok_or(NativeMaintenanceTemperatureFitError::Input)?;
    let calendar = NativePredecessorModelCalendar::new()
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let month_start = local_month_start(&calendar, &month)?;
    let cutoff = month_start
        .checked_sub(EMBARGO)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let (JsonValue::Array(training), JsonValue::Array(development)) = (
        field(value, "trainingRows").ok_or(NativeMaintenanceTemperatureFitError::Input)?,
        field(value, "developmentRows").ok_or(NativeMaintenanceTemperatureFitError::Input)?,
    ) else {
        return Err(NativeMaintenanceTemperatureFitError::RowCeiling);
    };
    // bound both populations under the source's shared ceiling
    if training
        .len()
        .checked_add(development.len())
        .is_none_or(|count| count > MAXIMUM_ROWS)
    {
        return Err(NativeMaintenanceTemperatureFitError::RowCeiling);
    }
    let training = admit_rows(training, "training", cutoff, &calendar)?;
    let development = admit_rows(development, "development", cutoff, &calendar)?;
    let input = FitInput {
        month,
        month_start,
        incumbent: field(value, "incumbentModel")
            .ok_or(NativeMaintenanceTemperatureFitError::Incumbent)?,
        training,
        development,
    };
    fit_temperature(input)
}

/// Admit every immutable row receipt and same-phase event identity.
fn admit_rows<'a>(
    values: &'a [JsonValue],
    _phase: &str,
    cutoff: i64,
    calendar: &NativePredecessorModelCalendar,
) -> Result<Vec<TemperatureRow<'a>>, NativeMaintenanceTemperatureFitError> {
    let mut seen = HashSet::with_capacity(values.len());
    let mut rows = Vec::with_capacity(values.len());
    // preserve source order while rejecting duplicated same-phase events
    for value in values {
        let row = admit_row(value, calendar)?;
        // require actual earlier immutable availability and opened development labels
        if !seen.insert(row.key.clone())
            || row.source_receipt >= cutoff
            || row.target_receipt >= cutoff
            || row.valid_at >= cutoff
            || text(value, "evidenceClass").as_deref() != Some("development")
        {
            return Err(NativeMaintenanceTemperatureFitError::Provenance);
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Decode one complete temperature fitter row without coercing nullable predictors.
fn admit_row<'a>(
    value: &'a JsonValue,
    calendar: &NativePredecessorModelCalendar,
) -> Result<TemperatureRow<'a>, NativeMaintenanceTemperatureFitError> {
    let key = text(value, "key")
        .filter(|key| !key.is_empty())
        .ok_or(NativeMaintenanceTemperatureFitError::Forecast)?;
    let run_text =
        text(value, "runInitializedAt").ok_or(NativeMaintenanceTemperatureFitError::Forecast)?;
    let valid_text =
        text(value, "validAt").ok_or(NativeMaintenanceTemperatureFitError::Forecast)?;
    let run_at = exact_instant(&run_text)?;
    let valid_at = exact_instant(&valid_text)?;
    let source_receipt = exact_instant(
        &text(value, "sourceReceiptAt").ok_or(NativeMaintenanceTemperatureFitError::Provenance)?,
    )?;
    let target_receipt = exact_instant(
        &text(value, "targetMaxReceiptAt")
            .ok_or(NativeMaintenanceTemperatureFitError::Provenance)?,
    )?;
    let model_lead = integer(value, "modelLeadHours")?;
    let operational_horizon = integer(value, "operationalHorizonHours")?;
    // bind the fixed delayed serving horizon to forecast chronology
    if !(7..=18).contains(&model_lead)
        || operational_horizon != model_lead - 6
        || valid_at.checked_sub(run_at) != Some(model_lead as i64 * HOUR)
    {
        return Err(NativeMaintenanceTemperatureFitError::Forecast);
    }
    let cohort = text(value, "cohort").ok_or(NativeMaintenanceTemperatureFitError::Forecast)?;
    let model_cycle = text(value, "modelCycle")
        .filter(|cycle| !cycle.is_empty())
        .ok_or(NativeMaintenanceTemperatureFitError::Forecast)?;
    let features = calendar
        .features(&valid_text)
        .map_err(|_| NativeMaintenanceTemperatureFitError::Calendar)?;
    let local_year = features
        .local_date
        .get(..4)
        .and_then(|year| year.parse::<i64>().ok())
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let local_day_ordinal = civil_day(local_year, features.month, features.local_day)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let state = field(value, "state").ok_or(NativeMaintenanceTemperatureFitError::State)?;
    Ok(TemperatureRow {
        actual: number(value, "actualTemperatureC")
            .ok_or(NativeMaintenanceTemperatureFitError::Provenance)?,
        cohort,
        key,
        local_date: features.local_date,
        local_day_ordinal,
        local_hour: features.hour,
        local_month: format!("{local_year:04}-{:02}", features.month),
        local_year,
        model_cycle,
        model_lead,
        operational_horizon,
        raw: number(value, "rawTemperatureC")
            .ok_or(NativeMaintenanceTemperatureFitError::Forecast)?,
        humidity: nullable_number(value, "rawRelativeHumidityPercent")?,
        wind: nullable_number(value, "rawWindSpeedMps")?,
        run_at,
        run_text,
        source_receipt,
        target_receipt,
        valid_at,
        valid_text,
        state,
    })
}

/// Fit, score, and select the two frozen candidate arms.
fn fit_temperature(input: FitInput<'_>) -> Result<JsonValue, NativeMaintenanceTemperatureFitError> {
    let dates = input
        .development
        .iter()
        .map(|row| row.local_date.as_str())
        .collect::<BTreeSet<_>>();
    let supported = input.development.len() >= 1_000 && dates.len() >= 60;
    let weights = normalized_event_weights(&input.development)?;
    let incumbent_results = infer_rows(
        &input.development,
        &HashMap::from([("frozen".to_owned(), (*input.incumbent).clone())]),
        true,
    )?;
    let raw_mae = weighted_mae(&input.development, &weights, None)?;
    let incumbent_mae = weighted_mae(&input.development, &weights, Some(&incumbent_results))?;
    let mut arms = Vec::with_capacity(2);
    let mut final_models = HashMap::new();
    // perform both final fits before applying the development winner screen
    for arm in [EXPANDING, TRAILING] {
        let final_fit = fit_candidate(&input.training, &input.month, arm, input.incumbent)?;
        let score_months = input
            .development
            .iter()
            .map(|row| row.local_month.clone())
            .collect::<BTreeSet<_>>();
        let mut score_models = HashMap::with_capacity(score_months.len());
        // fit each historical score month under its own earlier embargo
        for score_month in score_months {
            let score_fit = fit_candidate(&input.training, &score_month, arm, input.incumbent)?;
            score_models.insert(score_month, score_fit.model);
        }
        let results = infer_rows(&input.development, &score_models, false)?;
        let mae = weighted_mae(&input.development, &weights, Some(&results))?;
        let applied_count = results.iter().filter(|result| result.applied).count();
        let eligible = supported
            && final_fit.supported
            && applied_count > 0
            && mae <= raw_mae
            && mae <= incumbent_mae;
        let fallback_count = results.iter().filter(|result| !result.applied).count();
        // hide development loss until the fixed support floor is met
        let development_mae = if supported {
            JsonValue::Number(mae)
        } else {
            JsonValue::Null
        };
        let arm_value = object(vec![
            ("fitReceipt", final_fit.receipt.clone()),
            ("developmentMae", development_mae),
            ("developmentDates", JsonValue::Number(dates.len() as f64)),
            (
                "developmentRows",
                JsonValue::Number(input.development.len() as f64),
            ),
            ("fallbackCount", JsonValue::Number(fallback_count as f64)),
            ("eligible", JsonValue::Boolean(eligible)),
        ]);
        final_models.insert(arm, final_fit.model);
        arms.push((arm, arm_value, eligible, mae));
    }
    let selected = arms
        .iter()
        .filter(|(_, _, eligible, _)| *eligible)
        .min_by(|left, right| {
            left.3
                .partial_cmp(&right.3)
                .unwrap_or(Ordering::Equal)
                .then_with(|| arm_index(left.0).cmp(&arm_index(right.0)))
        })
        .map(|(arm, _, _, _)| *arm);
    let model = selected
        .map(|arm| {
            permanent_model(
                final_models
                    .get(arm)
                    .ok_or(NativeMaintenanceTemperatureFitError::Numeric)?,
                input.month_start,
                arms.iter()
                    .find(|candidate| candidate.0 == arm)
                    .map(|candidate| &candidate.1)
                    .ok_or(NativeMaintenanceTemperatureFitError::Numeric)?,
            )
        })
        .transpose()?
        .unwrap_or(JsonValue::Null);
    let arms_value = JsonValue::Object(
        arms.into_iter()
            .map(|(arm, value, _, _)| (arm.encode_utf16().collect(), value))
            .collect(),
    );
    // preserve explicit unscored metrics below the fixed support floor
    let development_raw_mae = if supported {
        JsonValue::Number(raw_mae)
    } else {
        JsonValue::Null
    };
    // preserve the incumbent metric under the identical support gate
    let development_incumbent_mae = if supported {
        JsonValue::Number(incumbent_mae)
    } else {
        JsonValue::Null
    };
    Ok(object(vec![
        ("contractVersion", string(OUTPUT_VERSION)),
        ("dueMonth", string(&input.month)),
        (
            "state",
            string(if selected.is_some() {
                "development_candidate"
            } else {
                "no_candidate"
            }),
        ),
        (
            "selectedArm",
            selected.map(string).unwrap_or(JsonValue::Null),
        ),
        ("model", model),
        ("arms", arms_value),
        ("confirmationOpened", JsonValue::Boolean(false)),
        ("servingChanged", JsonValue::Boolean(false)),
        ("developmentRawMae", development_raw_mae),
        ("developmentIncumbentMae", development_incumbent_mae),
    ]))
}

/// Return the frozen source preference for equal-loss arm selection.
fn arm_index(arm: &str) -> usize {
    match arm {
        EXPANDING => 1,
        TRAILING => 2,
        _ => usize::MAX,
    }
}

/// Select causal rows, fit one monthly model, and bind its immutable receipt.
fn fit_candidate(
    rows: &[TemperatureRow<'_>],
    month: &str,
    arm: &str,
    incumbent: &JsonValue,
) -> Result<CandidateFit, NativeMaintenanceTemperatureFitError> {
    let calendar = NativePredecessorModelCalendar::new()
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let month_start = local_month_start(&calendar, month)?;
    let cutoff = month_start
        .checked_sub(EMBARGO)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let cutoff_text = format_predecessor_source_utc(cutoff)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let first_local_day = civil_day_from_epoch(month_start, &calendar)?;
    let earliest = first_local_day
        .checked_sub(365)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let mut selected = Vec::new();
    let mut exclusions = BTreeMap::<&str, usize>::new();
    // retain source exclusion precedence and local-calendar trailing semantics
    for row in rows {
        if row.target_receipt >= cutoff
            || row
                .valid_at
                .checked_add(7 * HOUR)
                .is_none_or(|v| v > cutoff)
        {
            *exclusions.entry("target_after_cutoff").or_default() += 1;
            continue;
        }
        // constrain only the preregistered trailing arm
        if arm == TRAILING && row.local_day_ordinal < earliest {
            *exclusions.entry("outside_trailing_window").or_default() += 1;
            continue;
        }
        // reject undeclared arms before numerical work
        if arm != EXPANDING && arm != TRAILING {
            return Err(NativeMaintenanceTemperatureFitError::Input);
        }
        selected.push(row);
    }
    let fitted = fit_monthly_model(selected, month, &cutoff_text)?;
    let strength = fixed_strength_bands(incumbent, &cutoff_text)?;
    let learned = text(incumbent, "learnedStrengthContractVersion")
        .ok_or(NativeMaintenanceTemperatureFitError::Incumbent)?;
    let monthly = merge_model_material(&fitted.value, &learned, strength)?;
    let receipt = object(vec![
        ("arm", string(arm)),
        ("month", string(month)),
        ("modelSha256", string(&canonical_digest(&monthly)?)),
        (
            "trainingRows",
            JsonValue::Number(fitted.training_rows as f64),
        ),
        (
            "trainingDates",
            JsonValue::Number(fitted.training_dates as f64),
        ),
        ("trainingCutoffUtc", string(&cutoff_text)),
        (
            "latestTrainingValidAt",
            fitted
                .last_training_valid_at
                .as_deref()
                .map(string)
                .unwrap_or(JsonValue::Null),
        ),
        (
            "trainingKeySha256",
            string(&canonical_digest(&JsonValue::Array(
                fitted.training_keys.iter().map(|key| string(key)).collect(),
            ))?),
        ),
        (
            "exclusions",
            JsonValue::Object(
                exclusions
                    .into_iter()
                    .map(|(name, count)| {
                        (
                            name.encode_utf16().collect(),
                            JsonValue::Number(count as f64),
                        )
                    })
                    .collect(),
            ),
        ),
        (
            "strengthPolicy",
            string("frozen_incumbent_learned_strength"),
        ),
        (
            "originalStrengthBandReceipts",
            field(incumbent, "strengthBands")
                .ok_or(NativeMaintenanceTemperatureFitError::Incumbent)?
                .clone(),
        ),
        ("strengthRefitted", JsonValue::Boolean(false)),
        ("servingChanged", JsonValue::Boolean(false)),
    ]);
    Ok(CandidateFit {
        model: monthly,
        receipt,
        supported: fitted.supported,
    })
}

/// Fit the source's direct and adaptive robust ridge schemas from one shared population.
fn fit_monthly_model(
    mut rows: Vec<&TemperatureRow<'_>>,
    month: &str,
    cutoff_text: &str,
) -> Result<FittedModel, NativeMaintenanceTemperatureFitError> {
    let cutoff = exact_instant(cutoff_text)?;
    rows.retain(|row| row.cohort == COHORT && row.valid_at + 7 * HOUR <= cutoff);
    rows.sort_by(|left, right| left.key.cmp(&right.key));
    let dates = rows
        .iter()
        .map(|row| row.local_date.as_str())
        .collect::<BTreeSet<_>>();
    let supported = rows.len() >= 1_000 && dates.len() >= 60;
    let weights = event_weights_refs(&rows)?;
    let residuals = rows
        .iter()
        .map(|row| row.actual - row.raw)
        .collect::<Vec<_>>();
    let direct = if supported {
        JsonValue::Array(
            fit_coefficients(&rows, &residuals, &weights, 35, static_features)?
                .into_iter()
                .map(JsonValue::Number)
                .collect(),
        )
    } else {
        JsonValue::Null
    };
    let adaptive = if supported {
        JsonValue::Array(
            fit_coefficients(&rows, &residuals, &weights, 49, adaptive_features)?
                .into_iter()
                .map(JsonValue::Number)
                .collect(),
        )
    } else {
        JsonValue::Null
    };
    let last = rows
        .iter()
        .map(|row| row.valid_text.as_str())
        .max()
        .map(str::to_owned);
    let training_keys = rows.iter().map(|row| row.key.clone()).collect::<Vec<_>>();
    let value = object(vec![
        ("contractVersion", string(MONTHLY_MODEL_VERSION)),
        ("month", string(month)),
        ("cohort", string(COHORT)),
        ("scope", string(SCOPE)),
        ("supported", JsonValue::Boolean(supported)),
        ("directCoefficients", direct),
        ("adaptiveCoefficients", adaptive),
        ("trainingCutoffUtc", string(cutoff_text)),
    ]);
    Ok(FittedModel {
        value,
        supported,
        training_rows: rows.len(),
        training_dates: dates.len(),
        last_training_valid_at: last,
        training_keys,
    })
}

/// Freeze incumbent strength values while replacing only their causal training cutoff.
fn fixed_strength_bands(
    incumbent: &JsonValue,
    cutoff: &str,
) -> Result<JsonValue, NativeMaintenanceTemperatureFitError> {
    let JsonValue::Object(bands) =
        field(incumbent, "strengthBands").ok_or(NativeMaintenanceTemperatureFitError::Incumbent)?
    else {
        return Err(NativeMaintenanceTemperatureFitError::Incumbent);
    };
    let mut output = Vec::with_capacity(bands.len());
    // preserve every incumbent band key and its learned material
    for (name, band) in bands {
        let alpha = number(band, "alpha").ok_or(NativeMaintenanceTemperatureFitError::Incumbent)?;
        let supported =
            boolean(band, "supported").ok_or(NativeMaintenanceTemperatureFitError::Incumbent)?;
        output.push((
            name.clone(),
            object(vec![
                ("alpha", JsonValue::Number(alpha)),
                ("supported", JsonValue::Boolean(supported)),
                ("trainingCutoffUtc", string(cutoff)),
            ]),
        ));
    }
    Ok(JsonValue::Object(output))
}

/// Add the frozen learned-strength material to one fitted monthly model.
fn merge_model_material(
    fitted: &JsonValue,
    learned: &str,
    strength: JsonValue,
) -> Result<JsonValue, NativeMaintenanceTemperatureFitError> {
    let JsonValue::Object(fields) = fitted else {
        return Err(NativeMaintenanceTemperatureFitError::Numeric);
    };
    let mut fields = fields.clone();
    fields.push((
        "learnedStrengthContractVersion".encode_utf16().collect(),
        string(learned),
    ));
    fields.push(("strengthBands".encode_utf16().collect(), strength));
    Ok(JsonValue::Object(fields))
}

/// Convert the selected monthly fit into the permanent candidate material.
fn permanent_model(
    monthly: &JsonValue,
    month_start: i64,
    arm: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceTemperatureFitError> {
    let JsonValue::Object(fields) = monthly else {
        return Err(NativeMaintenanceTemperatureFitError::Numeric);
    };
    let receipt = field(arm, "fitReceipt").ok_or(NativeMaintenanceTemperatureFitError::Numeric)?;
    let latest = field(receipt, "latestTrainingValidAt")
        .ok_or(NativeMaintenanceTemperatureFitError::Numeric)?
        .clone();
    let mut output = fields
        .iter()
        .filter(|(name, _)| {
            !name.iter().copied().eq("month".encode_utf16())
                && !name.iter().copied().eq("contractVersion".encode_utf16())
        })
        .cloned()
        .collect::<Vec<_>>();
    output.push((
        "contractVersion".encode_utf16().collect(),
        string(PERMANENT_MODEL_VERSION),
    ));
    output.push((
        "effectiveFrom".encode_utf16().collect(),
        string(
            &format_predecessor_source_utc(month_start)
                .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?,
        ),
    ));
    output.push(("latestTrainingValidAt".encode_utf16().collect(), latest));
    Ok(JsonValue::Object(output))
}

/// Fit one fixed eight-step Huber-weighted ridge system.
fn fit_coefficients(
    rows: &[&TemperatureRow<'_>],
    residuals: &[f64],
    weights: &[f64],
    width: usize,
    features: fn(&TemperatureRow<'_>) -> Result<Vec<f64>, NativeMaintenanceTemperatureFitError>,
) -> Result<Vec<f64>, NativeMaintenanceTemperatureFitError> {
    let blas = openblas()?;
    let mut matrix = Vec::with_capacity(rows.len().saturating_mul(width));
    // materialize one schema at a time to keep peak memory bounded
    for row in rows {
        let values = features(row)?;
        // refuse feature-schema drift before indexing the flat matrix
        if values.len() != width || values.iter().any(|value| !value.is_finite()) {
            return Err(NativeMaintenanceTemperatureFitError::Numeric);
        }
        matrix.extend(values);
    }
    let penalty = 0.01 * numpy_pairwise_sum(weights);
    let mut coefficients = vec![0.0; width];
    let row_count =
        i64::try_from(rows.len()).map_err(|_| NativeMaintenanceTemperatureFitError::Numeric)?;
    let width_i64 =
        i64::try_from(width).map_err(|_| NativeMaintenanceTemperatureFitError::Numeric)?;
    // reproduce the frozen fixed iteration count without score-based early stopping
    for _ in 0..8 {
        let mut predictions = vec![0.0; rows.len()];
        // use the exact pinned gemv kernel used by numpy's matrix-vector product
        unsafe {
            (blas.dgemv)(
                CBLAS_ROW_MAJOR,
                CBLAS_NO_TRANSPOSE,
                row_count,
                width_i64,
                1.0,
                matrix.as_ptr(),
                width_i64,
                coefficients.as_ptr(),
                1,
                0.0,
                predictions.as_mut_ptr(),
                1,
            );
        }
        let mut weighted_matrix = vec![0.0; matrix.len()];
        let mut weighted_residuals = vec![0.0; rows.len()];
        // reproduce numpy's ordered scalar ufuncs before its dense products
        for index in 0..rows.len() {
            let error = residuals[index] - predictions[index];
            let robust = (1.5 / error.abs().max(1e-12)).min(1.0);
            let effective = weights[index] * robust;
            weighted_residuals[index] = effective * residuals[index];
            // multiply each matrix row by its exact effective event weight
            for column in 0..width {
                weighted_matrix[index * width + column] =
                    matrix[index * width + column] * effective;
            }
        }
        let mut normal = vec![0.0; width * width];
        let mut rhs = vec![0.0; width];
        // use the wheel's exact cblas products behind numpy matmul
        unsafe {
            (blas.dgemm)(
                CBLAS_ROW_MAJOR,
                CBLAS_TRANSPOSE,
                CBLAS_NO_TRANSPOSE,
                width_i64,
                width_i64,
                row_count,
                1.0,
                matrix.as_ptr(),
                width_i64,
                weighted_matrix.as_ptr(),
                width_i64,
                0.0,
                normal.as_mut_ptr(),
                width_i64,
            );
            (blas.dgemv)(
                CBLAS_ROW_MAJOR,
                CBLAS_TRANSPOSE,
                row_count,
                width_i64,
                1.0,
                matrix.as_ptr(),
                width_i64,
                weighted_residuals.as_ptr(),
                1,
                0.0,
                rhs.as_mut_ptr(),
                1,
            );
        }
        // leave the intercept unpenalized
        for diagonal in 1..width {
            normal[diagonal * width + diagonal] += penalty;
        }
        coefficients = solve_openblas(blas, normal, rhs, width)?;
    }
    // prohibit silent nonfinite candidate material
    if coefficients.iter().any(|value| !value.is_finite()) {
        return Err(NativeMaintenanceTemperatureFitError::Numeric);
    }
    Ok(coefficients)
}

/// Solve one dense system through the exact pinned numpy LAPACK kernel.
fn solve_openblas(
    blas: &OpenBlas,
    matrix: Vec<f64>,
    mut rhs: Vec<f64>,
    width: usize,
) -> Result<Vec<f64>, NativeMaintenanceTemperatureFitError> {
    let width_i64 =
        i64::try_from(width).map_err(|_| NativeMaintenanceTemperatureFitError::Numeric)?;
    let mut column_major = vec![0.0; matrix.len()];
    // copy the c-order numpy matrix into lapack's column-major storage
    for row in 0..width {
        for column in 0..width {
            column_major[column * width + row] = matrix[row * width + column];
        }
    }
    let mut pivots = vec![0i64; width];
    let mut info = 0i64;
    let one = 1i64;
    // solve one right-hand side through numpy's ilp64 dgesv entrypoint
    unsafe {
        (blas.dgesv)(
            &width_i64,
            &one,
            column_major.as_mut_ptr(),
            &width_i64,
            pivots.as_mut_ptr(),
            rhs.as_mut_ptr(),
            &width_i64,
            &mut info,
        );
    }
    // match numpy's refusal of a singular or invalid dense solve
    if info != 0 {
        return Err(NativeMaintenanceTemperatureFitError::Numeric);
    }
    Ok(rhs)
}

/// Build the frozen thirty-five-column static feature schema.
fn static_features(
    row: &TemperatureRow<'_>,
) -> Result<Vec<f64>, NativeMaintenanceTemperatureFitError> {
    let year_start =
        civil_day(row.local_year, 1, 1).ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let next_start = civil_day(row.local_year + 1, 1, 1)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let annual = std::f64::consts::TAU
        * ((row.local_day_ordinal - year_start) as f64 + f64::from(row.local_hour) / 24.0)
        / (next_start - year_start) as f64;
    let daily = std::f64::consts::TAU * f64::from(row.local_hour) / 24.0;
    let annual_basis = [
        annual.sin(),
        annual.cos(),
        (2.0 * annual).sin(),
        (2.0 * annual).cos(),
    ];
    let daily_basis = [
        daily.sin(),
        daily.cos(),
        (2.0 * daily).sin(),
        (2.0 * daily).cos(),
    ];
    let temperature = (row.raw - 10.0) / 10.0;
    let humidity = row.humidity.map_or(0.0, |value| (value - 75.0) / 25.0);
    let wind = row.wind.map_or(0.0, |value| (value - 2.0) / 3.0);
    let mut values = vec![1.0];
    values.extend(annual_basis);
    values.extend(daily_basis);
    // append smooth seasonal changes in the daily cycle
    for annual_value in annual_basis.into_iter().take(2) {
        for daily_value in daily_basis.into_iter().take(2) {
            values.push(annual_value * daily_value);
        }
    }
    values.extend([
        temperature,
        temperature * temperature,
        humidity,
        wind,
        bool_number(row.humidity.is_none()),
        bool_number(row.wind.is_none()),
    ]);
    // append forecast-dependent daily and seasonal bias terms
    for value in [
        annual_basis[0],
        annual_basis[1],
        daily_basis[0],
        daily_basis[1],
    ] {
        values.push(temperature * value);
    }
    values.extend([
        humidity * daily_basis[0],
        humidity * daily_basis[1],
        wind * daily_basis[0],
        wind * daily_basis[1],
    ]);
    let lead = row.model_lead as f64 / 18.0;
    let cycle = std::f64::consts::TAU * (row.run_at.rem_euclid(DAY) / HOUR) as f64 / 24.0;
    values.extend([
        lead,
        lead * lead,
        lead * daily.sin(),
        lead * daily.cos(),
        lead * temperature,
        cycle.sin(),
        cycle.cos(),
        bool_number(row.model_cycle == "50r1"),
    ]);
    Ok(values)
}

/// Append the frozen fourteen-column recent-error feature schema.
fn adaptive_features(
    row: &TemperatureRow<'_>,
) -> Result<Vec<f64>, NativeMaintenanceTemperatureFitError> {
    let mut values = static_features(row)?;
    let state = validate_state(row)?;
    let short = state.b24.unwrap_or(0.0);
    let long = state.b72.unwrap_or(0.0);
    let difference = match (state.b24, state.b72) {
        (Some(short), Some(long)) => short - long,
        _ => 0.0,
    };
    let signed = [short / 3.0, long / 3.0, difference / 3.0];
    values.extend([
        signed[0],
        signed[1],
        signed[2],
        state.mad72.unwrap_or(0.0) / 3.0,
        state.n24 as f64 / 24.0,
        state.n72 as f64 / 72.0,
        bool_number(state.b24.is_none()),
        bool_number(state.b72.is_none()),
    ]);
    // retain both preregistered exponential horizon interactions
    for denominator in [6.0, 18.0] {
        let decay = (-(row.operational_horizon as f64) / denominator).exp();
        for value in signed {
            values.push(value * decay);
        }
    }
    Ok(values)
}

struct ValidatedState {
    b24: Option<f64>,
    b72: Option<f64>,
    mad72: Option<f64>,
    n24: usize,
    n72: usize,
}

/// Validate one frozen rolling-error state against its forecast initialization.
fn validate_state(
    row: &TemperatureRow<'_>,
) -> Result<ValidatedState, NativeMaintenanceTemperatureFitError> {
    let state = row.state;
    // preserve provider and initialization isolation
    if !matches!(state, JsonValue::Object(_))
        || text(state, "cohort").as_deref() != Some(row.cohort.as_str())
        || exact_instant(
            &text(state, "targetRunInitializedAt")
                .ok_or(NativeMaintenanceTemperatureFitError::State)?,
        )? != row.run_at
        || exact_instant(
            &text(state, "windowEndValidAt").ok_or(NativeMaintenanceTemperatureFitError::State)?,
        )? != row.run_at - 7 * HOUR
    {
        return Err(NativeMaintenanceTemperatureFitError::State);
    }
    let n24 = integer(state, "n24")?;
    let n72 = integer(state, "n72")?;
    let local_dates = integer(state, "localDates")?;
    // bind all support counters to their physical windows
    if n24 > 24 || n72 < n24 || n72 > 72 || local_dates > n72 {
        return Err(NativeMaintenanceTemperatureFitError::State);
    }
    let JsonValue::Array(source_keys) =
        field(state, "sourceKeys").ok_or(NativeMaintenanceTemperatureFitError::State)?
    else {
        return Err(NativeMaintenanceTemperatureFitError::State);
    };
    let mut unique = HashSet::with_capacity(source_keys.len());
    // require one nonempty unique audit key per selected hour
    for key in source_keys {
        let JsonValue::String(units) = key else {
            return Err(NativeMaintenanceTemperatureFitError::State);
        };
        let key = String::from_utf16(units)
            .ok()
            .filter(|key| !key.is_empty())
            .ok_or(NativeMaintenanceTemperatureFitError::State)?;
        // reject duplicate source-hour authority
        if !unique.insert(key) {
            return Err(NativeMaintenanceTemperatureFitError::State);
        }
    }
    // bind the source-key population to long-window support
    if source_keys.len() != n72 {
        return Err(NativeMaintenanceTemperatureFitError::State);
    }
    let maximum_valid = nullable_text(state, "maximumSourceValidAt")?;
    let maximum_run = nullable_text(state, "maximumSourceRunInitializedAt")?;
    // distinguish truly empty state from populated source receipts
    if n72 == 0 {
        if maximum_valid.is_some() || maximum_run.is_some() {
            return Err(NativeMaintenanceTemperatureFitError::State);
        }
    } else if exact_instant(
        maximum_valid
            .as_deref()
            .ok_or(NativeMaintenanceTemperatureFitError::State)?,
    )? > row.run_at - 7 * HOUR
        || exact_instant(
            maximum_run
                .as_deref()
                .ok_or(NativeMaintenanceTemperatureFitError::State)?,
        )? >= row.run_at
    {
        return Err(NativeMaintenanceTemperatureFitError::State);
    }
    let short_supported = n24 >= 6;
    let long_supported = n72 >= 24;
    let expected_supported = short_supported && long_supported && local_dates >= 2;
    // recompute the source support flag
    if boolean(state, "supported") != Some(expected_supported) {
        return Err(NativeMaintenanceTemperatureFitError::State);
    }
    let b24 = nullable_number(state, "b24C")?;
    let b72 = nullable_number(state, "b72C")?;
    let mad72 = nullable_number(state, "mad72C")?;
    // validate supported and unsupported statistic shapes separately
    if short_supported != b24.is_some()
        || long_supported != b72.is_some()
        || long_supported != mad72.is_some()
        || b24.is_some_and(|value| value.abs() > 6.0)
        || b72.is_some_and(|value| value.abs() > 6.0)
        || mad72.is_some_and(|value| !(0.0..=6.0).contains(&value))
    {
        return Err(NativeMaintenanceTemperatureFitError::State);
    }
    Ok(ValidatedState {
        b24,
        b72,
        mad72,
        n24,
        n72,
    })
}

/// Evaluate development rows through the existing admitted native inference owner.
fn infer_rows(
    rows: &[TemperatureRow<'_>],
    models: &HashMap<String, JsonValue>,
    frozen: bool,
) -> Result<Vec<InferenceResult>, NativeMaintenanceTemperatureFitError> {
    let mut results = Vec::with_capacity(rows.len());
    // retain the source's bounded inference batch size
    for batch in rows.chunks(256) {
        let mut inputs = Vec::with_capacity(batch.len());
        // pass only the runtime-declared forecast and state fields
        for row in batch {
            let model = models
                .get(if frozen { "frozen" } else { &row.local_month })
                .ok_or(NativeMaintenanceTemperatureFitError::Numeric)?;
            let forecast = object(vec![
                ("cohort", string(&row.cohort)),
                ("key", string(&row.key)),
                ("modelCycle", string(&row.model_cycle)),
                ("modelLeadHours", JsonValue::Number(row.model_lead as f64)),
                (
                    "rawRelativeHumidityPercent",
                    row.humidity
                        .map(JsonValue::Number)
                        .unwrap_or(JsonValue::Null),
                ),
                ("rawTemperatureC", JsonValue::Number(row.raw)),
                (
                    "rawWindSpeedMps",
                    row.wind.map(JsonValue::Number).unwrap_or(JsonValue::Null),
                ),
                ("runInitializedAt", string(&row.run_text)),
                ("validAt", string(&row.valid_text)),
            ]);
            inputs.push(object(vec![
                ("forecast", forecast),
                ("recentErrorState", (*row.state).clone()),
                ("model", model.clone()),
            ]));
        }
        let request = object(vec![
            ("operation", string("infer")),
            ("inputs", JsonValue::Array(inputs)),
        ]);
        let JsonValue::Array(batch_results) =
            crate::native_maintenance_temperature_fit_bridge::evaluate_native_temperature_fit_batch(
                &request,
            )
            .ok_or(NativeMaintenanceTemperatureFitError::Numeric)?
        else {
            return Err(NativeMaintenanceTemperatureFitError::Numeric);
        };
        // decode every ordered runtime result without accepting missing rows
        for result in batch_results {
            results.push(InferenceResult {
                applied: boolean(&result, "applied")
                    .ok_or(NativeMaintenanceTemperatureFitError::Numeric)?,
                prediction: number(&result, "predictionTemperatureC")
                    .ok_or(NativeMaintenanceTemperatureFitError::Numeric)?,
            });
        }
    }
    Ok(results)
}

/// Compute the shared equal-date, equal-hour, equal-event source weights.
fn normalized_event_weights(
    rows: &[TemperatureRow<'_>],
) -> Result<Vec<f64>, NativeMaintenanceTemperatureFitError> {
    let refs = rows.iter().collect::<Vec<_>>();
    let mut weights = event_weights_refs(&refs)?;
    let total = numpy_pairwise_sum(&weights);
    // preserve empty development as an unscored zero sum
    if !weights.is_empty() {
        if total <= 0.0 || !total.is_finite() {
            return Err(NativeMaintenanceTemperatureFitError::Numeric);
        }
        for weight in &mut weights {
            *weight /= total;
        }
    }
    Ok(weights)
}

/// Assign each local date equal mass and each represented valid hour equal date mass.
fn event_weights_refs(
    rows: &[&TemperatureRow<'_>],
) -> Result<Vec<f64>, NativeMaintenanceTemperatureFitError> {
    let mut grouped = HashMap::<&str, HashMap<i64, usize>>::new();
    // count repeated events inside each represented hour and date
    for row in rows {
        *grouped
            .entry(&row.local_date)
            .or_default()
            .entry(row.valid_at)
            .or_default() += 1;
    }
    rows.iter()
        .map(|row| {
            let hours = grouped
                .get(row.local_date.as_str())
                .ok_or(NativeMaintenanceTemperatureFitError::Numeric)?;
            let events = hours
                .get(&row.valid_at)
                .ok_or(NativeMaintenanceTemperatureFitError::Numeric)?;
            Ok(1.0 / (hours.len() * events) as f64)
        })
        .collect()
}

/// Compute one weighted absolute error over the exact paired population.
fn weighted_mae(
    rows: &[TemperatureRow<'_>],
    weights: &[f64],
    results: Option<&[InferenceResult]>,
) -> Result<f64, NativeMaintenanceTemperatureFitError> {
    // require exact population alignment before combining outcomes
    if rows.len() != weights.len() || results.is_some_and(|values| values.len() != rows.len()) {
        return Err(NativeMaintenanceTemperatureFitError::Numeric);
    }
    let terms = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let prediction = results.map_or(row.raw, |values| values[index].prediction);
            weights[index] * (prediction - row.actual).abs()
        })
        .collect::<Vec<_>>();
    let value = python_compensated_sum(&terms);
    value
        .is_finite()
        .then_some(value)
        .ok_or(NativeMaintenanceTemperatureFitError::Numeric)
}

/// Reproduce CPython 3.14 built-in float sum's compensated accumulation.
fn python_compensated_sum(values: &[f64]) -> f64 {
    let mut high = 0.0;
    let mut low = 0.0;
    // preserve source generator order while retaining every lost low component
    for &value in values {
        let total = high + value;
        low += if high.abs() >= value.abs() {
            (high - total) + value
        } else {
            (value - total) + high
        };
        high = total;
    }
    high + low
}

/// Resolve exact LA local midnight with the same bounded offset correction owner.
fn local_month_start(
    calendar: &NativePredecessorModelCalendar,
    month: &str,
) -> Result<i64, NativeMaintenanceTemperatureFitError> {
    let bytes = month.as_bytes();
    // require the source's canonical four-digit calendar month
    if bytes.len() != 7
        || bytes.get(4) != Some(&b'-')
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..].iter().all(u8::is_ascii_digit)
    {
        return Err(NativeMaintenanceTemperatureFitError::Calendar);
    }
    let month_number = month[5..]
        .parse::<u8>()
        .ok()
        .filter(|value| (1..=12).contains(value))
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let target_text = format!("{month}-01T00:00:00.000Z");
    let target = parse_predecessor_source_utc(&target_text)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?
        .epoch_milliseconds;
    let guess_text = format!("{month}-01T08:00:00.000Z");
    let mut guess = parse_predecessor_source_utc(&guess_text)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?
        .epoch_milliseconds;
    // reproduce the source's bounded wall-clock correction
    for _ in 0..3 {
        let instant = format_predecessor_source_utc(guess)
            .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
        let offset = calendar
            .features(&instant)
            .map_err(|_| NativeMaintenanceTemperatureFitError::Calendar)?
            .offset_seconds;
        let observed = guess + i64::from(offset) * 1_000;
        guess += target - observed;
    }
    let resolved = format_predecessor_source_utc(guess)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let local = calendar
        .features(&resolved)
        .map_err(|_| NativeMaintenanceTemperatureFitError::Calendar)?;
    // prove the requested date and month survived offset resolution
    if local.local_date != format!("{month}-01") || local.month != month_number || local.hour != 0 {
        return Err(NativeMaintenanceTemperatureFitError::Calendar);
    }
    Ok(guess)
}

/// Return the local civil-day ordinal at one admitted UTC epoch.
fn civil_day_from_epoch(
    epoch: i64,
    calendar: &NativePredecessorModelCalendar,
) -> Result<i64, NativeMaintenanceTemperatureFitError> {
    let instant = format_predecessor_source_utc(epoch)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    let local = calendar
        .features(&instant)
        .map_err(|_| NativeMaintenanceTemperatureFitError::Calendar)?;
    civil_day(local.local_astronomical_year, local.month, local.local_day)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)
}

/// Convert one Gregorian date to a stable day ordinal.
fn civil_day(year: i64, month: u8, day: u8) -> Option<i64> {
    // reject normalized month and day spellings before arithmetic
    if !(1..=12).contains(&month) || !(1..=days_in_month(year, month)?).contains(&day) {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era)
}

/// Return the exact Gregorian month width.
fn days_in_month(year: i64, month: u8) -> Option<u8> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => return None,
    })
}

/// Parse one exact canonical millisecond UTC instant.
fn exact_instant(value: &str) -> Result<i64, NativeMaintenanceTemperatureFitError> {
    let parsed = parse_predecessor_source_utc(value)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)?;
    (parsed.utc == value)
        .then_some(parsed.epoch_milliseconds)
        .ok_or(NativeMaintenanceTemperatureFitError::Calendar)
}

/// Decode one finite nullable numerical field.
fn nullable_number(
    value: &JsonValue,
    key: &str,
) -> Result<Option<f64>, NativeMaintenanceTemperatureFitError> {
    match field(value, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::Number(value)) if value.is_finite() => Ok(Some(*value)),
        _ => Err(NativeMaintenanceTemperatureFitError::Forecast),
    }
}

/// Decode one nullable UTF-16 string field.
fn nullable_text(
    value: &JsonValue,
    key: &str,
) -> Result<Option<String>, NativeMaintenanceTemperatureFitError> {
    match field(value, key) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .map(Some)
            .map_err(|_| NativeMaintenanceTemperatureFitError::State),
        _ => Err(NativeMaintenanceTemperatureFitError::State),
    }
}

/// Decode one source integer without admitting booleans or fractional numbers.
fn integer(value: &JsonValue, key: &str) -> Result<usize, NativeMaintenanceTemperatureFitError> {
    let value = number(value, key).ok_or(NativeMaintenanceTemperatureFitError::Forecast)?;
    // retain only nonnegative safe integers
    if value.fract() != 0.0 || !(0.0..=9_007_199_254_740_991.0).contains(&value) {
        return Err(NativeMaintenanceTemperatureFitError::Forecast);
    }
    Ok(value as usize)
}

/// Decode one exact boolean field.
fn boolean(value: &JsonValue, key: &str) -> Option<bool> {
    match field(value, key)? {
        JsonValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

/// Hash recursively sorted canonical bytes without the file-framing line feed.
fn canonical_digest(value: &JsonValue) -> Result<String, NativeMaintenanceTemperatureFitError> {
    let bytes = encode_python_canonical(value)?;
    Ok(format_digest(NativeWirePrimitives.sha256(&bytes)))
}

#[derive(Clone, Copy)]
enum PythonNumberKind {
    Auto,
    Float,
}

/// Encode the source's compact sorted `json.dumps` form without report framing.
fn encode_python_canonical(
    value: &JsonValue,
) -> Result<Vec<u8>, NativeMaintenanceTemperatureFitError> {
    let mut output = Vec::new();
    write_python_value(value, PythonNumberKind::Auto, &mut output)?;
    Ok(output)
}

/// Write one recursively sorted source JSON value with field-aware number identity.
fn write_python_value(
    value: &JsonValue,
    number_kind: PythonNumberKind,
    output: &mut Vec<u8>,
) -> Result<(), NativeMaintenanceTemperatureFitError> {
    match value {
        JsonValue::Null => output.extend_from_slice(b"null"),
        JsonValue::Boolean(true) => output.extend_from_slice(b"true"),
        JsonValue::Boolean(false) => output.extend_from_slice(b"false"),
        JsonValue::Number(value) => {
            output.extend_from_slice(python_number(*value, number_kind)?.as_bytes());
        }
        JsonValue::String(units) => {
            let value = String::from_utf16(units)
                .map_err(|_| NativeMaintenanceTemperatureFitError::Codec)?;
            output.extend_from_slice(
                serde_json::to_string(&value)
                    .map_err(|_| NativeMaintenanceTemperatureFitError::Codec)?
                    .as_bytes(),
            );
        }
        JsonValue::Array(values) => {
            output.push(b'[');
            // preserve array order while retaining its owning field's numeric kind
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_python_value(value, number_kind, output)?;
            }
            output.push(b']');
        }
        JsonValue::Object(fields) => {
            let mut fields = fields.iter().collect::<Vec<_>>();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            output.push(b'{');
            // emit every unique source key in sorted compact form
            for (index, (units, value)) in fields.into_iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                let key = String::from_utf16(units)
                    .map_err(|_| NativeMaintenanceTemperatureFitError::Codec)?;
                output.extend_from_slice(
                    serde_json::to_string(&key)
                        .map_err(|_| NativeMaintenanceTemperatureFitError::Codec)?
                        .as_bytes(),
                );
                output.push(b':');
                let kind = match key.as_str() {
                    "directCoefficients"
                    | "adaptiveCoefficients"
                    | "developmentMae"
                    | "developmentRawMae"
                    | "developmentIncumbentMae" => PythonNumberKind::Float,
                    _ => PythonNumberKind::Auto,
                };
                write_python_value(value, kind, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

/// Render one finite Rust binary64 using Python's shortest float exponent spelling.
fn python_number(
    value: f64,
    kind: PythonNumberKind,
) -> Result<String, NativeMaintenanceTemperatureFitError> {
    // reject values that source json.dumps refuses under allow_nan false
    if !value.is_finite() {
        return Err(NativeMaintenanceTemperatureFitError::Numeric);
    }
    // retain generated integer fields and integer-valued incumbent alpha material
    if matches!(kind, PythonNumberKind::Auto)
        && value.fract() == 0.0
        && value.abs() <= 9_007_199_254_740_991.0
    {
        return Ok((value as i64).to_string());
    }
    let source =
        serde_json::to_string(&value).map_err(|_| NativeMaintenanceTemperatureFitError::Codec)?;
    let Some((coefficient, exponent)) = source.split_once(['e', 'E']) else {
        return Ok(if source.contains('.') {
            source
        } else {
            format!("{source}.0")
        });
    };
    let exponent = exponent
        .parse::<i32>()
        .map_err(|_| NativeMaintenanceTemperatureFitError::Codec)?;
    Ok(format!(
        "{coefficient}e{}{absolute:02}",
        if exponent >= 0 { '+' } else { '-' },
        absolute = exponent.unsigned_abs()
    ))
}

/// Load only the manifest-pinned numpy OpenBLAS bytes used by the predecessor fit.
fn openblas() -> Result<&'static OpenBlas, NativeMaintenanceTemperatureFitError> {
    OPENBLAS
        .get_or_init(load_openblas)
        .as_ref()
        .ok_or(NativeMaintenanceTemperatureFitError::NumericalRuntime)
}

/// Verify and dynamically bind the exact reviewed ILP64 dense-kernel ABI.
fn load_openblas() -> Option<OpenBlas> {
    #[cfg(test)]
    let paths = [SANDBOX_OPENBLAS, HOST_OPENBLAS].as_slice();
    #[cfg(not(test))]
    let paths = [SANDBOX_OPENBLAS].as_slice();
    // prefer the isolated production mount before the fixed workstation oracle path
    for &path in paths {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        // bind both reviewed size and digest before executing any library code
        if bytes.len() != 24_325_657
            || format_digest(NativeWirePrimitives.sha256(&bytes)) != OPENBLAS_SHA256
        {
            continue;
        }
        let Ok(path) = CString::new(path) else {
            continue;
        };
        // safety: the verified library handle is intentionally retained for process lifetime
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            continue;
        }
        let dgemm_name = c"scipy_cblas_dgemm64_";
        let dgemv_name = c"scipy_cblas_dgemv64_";
        let dgesv_name = c"scipy_dgesv_64_";
        let threads_name = c"scipy_openblas_set_num_threads64_";
        // safety: symbol names and concrete signatures are fixed by the reviewed wheel ABI
        let (dgemm, dgemv, dgesv, threads) = unsafe {
            let dgemm = libc::dlsym(handle, dgemm_name.as_ptr());
            let dgemv = libc::dlsym(handle, dgemv_name.as_ptr());
            let dgesv = libc::dlsym(handle, dgesv_name.as_ptr());
            let threads = libc::dlsym(handle, threads_name.as_ptr());
            if dgemm.is_null() || dgemv.is_null() || dgesv.is_null() || threads.is_null() {
                libc::dlclose(handle);
                continue;
            }
            (
                std::mem::transmute::<*mut libc::c_void, Dgemm>(dgemm),
                std::mem::transmute::<*mut libc::c_void, Dgemv>(dgemv),
                std::mem::transmute::<*mut libc::c_void, Dgesv>(dgesv),
                std::mem::transmute::<*mut libc::c_void, SetThreads>(threads),
            )
        };
        // safety: the reviewed setter takes one ordinary c int and has no borrowed output
        unsafe { threads(1) };
        return Some(OpenBlas {
            dgemm,
            dgemv,
            dgesv,
        });
    }
    None
}

/// Reproduce numpy's fixed-block pairwise reduction used by ndarray sum.
fn numpy_pairwise_sum(values: &[f64]) -> f64 {
    const BLOCK: usize = 128;
    // retain the source's direct small-vector accumulation
    if values.len() < 8 {
        return values.iter().fold(-0.0, |total, value| total + value);
    }
    // use eight accumulators inside one pairwise block
    if values.len() <= BLOCK {
        let mut partial = [
            values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7],
        ];
        let limit = values.len() - values.len() % 8;
        let mut index = 8;
        while index < limit {
            // preserve numpy's eight-way unrolled addition order
            for lane in 0..8 {
                partial[lane] += values[index + lane];
            }
            index += 8;
        }
        let mut total = ((partial[0] + partial[1]) + (partial[2] + partial[3]))
            + ((partial[4] + partial[5]) + (partial[6] + partial[7]));
        // append only the short remainder after the balanced lanes
        while index < values.len() {
            total += values[index];
            index += 1;
        }
        return total;
    }
    let mut split = values.len() / 2;
    split -= split % 8;
    numpy_pairwise_sum(&values[..split]) + numpy_pairwise_sum(&values[split..])
}

/// Convert one source boolean indicator into its exact floating feature value.
fn bool_number(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}
