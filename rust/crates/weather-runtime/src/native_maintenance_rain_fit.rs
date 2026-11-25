//! Native rain maintenance fitting over the pinned XGBoost C ABI.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::ptr;
use std::slice;
use std::sync::OnceLock;

use jiff::{Timestamp, tz::TimeZone};
use sha2::{Digest, Sha256};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_policy_rain::{
    create_native_rain_maintenance_policy_row, evaluate_native_rain_maintenance_development_v3,
};
use crate::native_open_meteo_values::load_named_timezone;
use crate::{NativePredecessorModelCalendar, NativeWirePrimitives};

pub const NATIVE_MAINTENANCE_RAIN_FIT_VERSION: &str = "native-maintenance-rain-fit/v1";
pub const NATIVE_MAINTENANCE_RAIN_XGBOOST_PATH: &str = "/runtime/xgboost/lib/libxgboost.so";
pub const NATIVE_MAINTENANCE_RAIN_XGBOOST_SHA256: &str =
    "2850cc04daff59421d9cb2caefbff5ab064c78795802b403a5b7ee3f7e6b8eb4";

const FIT_INPUT_VERSION: &str = "rain-maintenance-fit-input/v3";
const FIT_OUTPUT_VERSION: &str = "rain-maintenance-fit/v3";
const CONTROL_INPUT_VERSION: &str = "rain-control-reference-fit-input/v1";
const CONTROL_OUTPUT_VERSION: &str = "rain-control-reference-fit/v1";
const POPULATION_VERSION: &str = "rain-maintenance-development-population/v3";
const ARTIFACT_VERSION: &str = "rain-maintenance-artifact/v2";
const HURDLE_VERSION: &str = "rain-hurdle-calibration/v1";
const FEATURE_COUNT: usize = 107;
const MAXIMUM_ROWS: usize = 250_000;
const MAXIMUM_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const TRAINING_START_HOUR: i64 = 475_104;
const DAY_HOURS: i64 = 24;
const GRID: [&str; 7] = [
    "R0_exact_refit",
    "R1_winter_scale_0_90",
    "R2_winter_scale_0_95",
    "R3_spring_wet_logit_plus_0_20",
    "R4_summer_wet_logit_plus_0_20",
    "R5_nested_cumulative_min",
    "R6_heavy_raw_blend_0_25",
];
const THRESHOLDS: [f64; 3] = [0.1, 1.0, 2.5];
const HEAD_NAMES: [&str; 4] = ["0.1", "1.0", "2.5", "amount"];
const MODEL_FILES: [&str; 4] = [
    "event-0.1.json",
    "event-1.0.json",
    "event-2.5.json",
    "amount.json",
];
const RETAINED_RAIN_ARTIFACT: &[u8] =
    include_bytes!("../../weather-contracts/retained-rain-hurdle-wind.json");
const OPENBLAS_SHA256: &str = "432e1c1739731468ba305771af96773939e43016643e2d630f58cb45b6670e96";
const SANDBOX_OPENBLAS: &str = "/runtime/numpy.libs/libscipy_openblas64_-f48b354e.so";
#[cfg(test)]
const HOST_OPENBLAS: &str = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14/site-packages/numpy.libs/libscipy_openblas64_-f48b354e.so";
type Ddot = unsafe extern "C" fn(i64, *const f64, i64, *const f64, i64) -> f64;
type SetOpenBlasThreads = unsafe extern "C" fn(c_int);
static OPENBLAS_DDOT: OnceLock<Option<Ddot>> = OnceLock::new();
static LOS_ANGELES: OnceLock<Option<TimeZone>> = OnceLock::new();

/// Closed failures for native rain fitting and its fixed numerical runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceRainFitError {
    Input,
    Contract,
    Clock,
    Population,
    Numeric,
    Runtime,
    Model,
    Policy,
    Codec,
    Output,
}

impl Display for NativeMaintenanceRainFitError {
    /// Render one stable diagnostic without leaking private fit material.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Input => "rain fit input is invalid",
            Self::Contract => "rain fit contract is unsupported",
            Self::Clock => "rain fit clock is invalid",
            Self::Population => "rain fit population is invalid",
            Self::Numeric => "rain fit numerical input is invalid",
            Self::Runtime => "rain fit numerical runtime is unavailable",
            Self::Model => "rain fit model operation failed",
            Self::Policy => "rain fit development policy failed",
            Self::Codec => "rain fit json codec failed",
            Self::Output => "rain fit output exceeded its boundary",
        })
    }
}

impl std::error::Error for NativeMaintenanceRainFitError {}

#[derive(Clone)]
struct RainRow {
    actual: f64,
    evaluation_row: Option<JsonValue>,
    features: Vec<f32>,
    gauge_count: usize,
    hour: i64,
    initialized: i64,
    key: String,
    operational_horizon_hours: u64,
    persistence: Option<f64>,
    raw: f64,
    raw_target_hour_temperature_c: f64,
    run_initialized_at: String,
    source_row_sha256: String,
    target_row_sha256: String,
    valid_at: String,
}

#[derive(Clone, Copy)]
struct MonthMasks {
    calibration_start: i64,
    calibration_stop: i64,
    decision_start: i64,
    decision_stop: i64,
    fit_stop: i64,
}

#[derive(Clone, Copy)]
struct Support {
    dates: usize,
    hours: usize,
    rows: usize,
    wet_dates: usize,
    wet_hours: usize,
}

#[derive(Clone, Copy)]
struct EffectiveSupport {
    dates: f64,
    wet_dates: f64,
}

#[derive(Clone, Copy)]
struct EventMetrics {
    csi: f64,
    far: f64,
    pod: f64,
}

#[derive(Clone)]
struct EventRule {
    candidate: Option<EventMetrics>,
    cutoff: Option<f64>,
    head: &'static str,
    raw: EventMetrics,
    reason: &'static str,
    support: HeadSupport,
    target_recall: Option<f64>,
    threshold: f64,
}

#[derive(Clone, Copy)]
struct HeadSupport {
    positive_dates: usize,
    positive_hours: usize,
    positive_rows: usize,
    rows: usize,
}

#[derive(Clone)]
struct ScalarCalibration {
    achieved_mean: f64,
    high_mean: f64,
    low_mean: f64,
    residual: f64,
    scale: f64,
    status: &'static str,
    target_mean: f64,
    tolerance: f64,
}

#[derive(Clone)]
struct CategoryCalibration {
    calibration: Option<ScalarCalibration>,
    observed_dry_rows: usize,
    observed_wet_dates: usize,
    observed_wet_hours: usize,
    observed_wet_rows: usize,
    predicted_dates: usize,
    predicted_hours: usize,
    reason: &'static str,
    rows: usize,
    scale: f64,
    supported: bool,
}

#[derive(Clone)]
struct WetSelection {
    candidate_scores: usize,
    feasible_scores: usize,
    old_nested_metrics: EventMetrics,
    positive_dates: usize,
    positive_hours: usize,
    raw_metrics: EventMetrics,
    reason: &'static str,
    selected_cutoff: Option<f64>,
    selected_metrics: EventMetrics,
}

#[derive(Clone)]
struct HurdleCalibration {
    calibration_rows: usize,
    calibration_stop: i64,
    categories: [CategoryCalibration; 3],
    dates: usize,
    effective: EffectiveSupport,
    global: ScalarCalibration,
    observed_wet_dates: usize,
    observed_wet_hours: usize,
    observed_wet_rows: usize,
    rows: usize,
    rules: Vec<EventRule>,
    uniform_rules: Vec<EventRule>,
    wet_selection: WetSelection,
}

struct ModelHead {
    booster: Option<XgBooster>,
    bytes: Option<Vec<u8>>,
    objective: &'static str,
    reason: &'static str,
    support: HeadSupport,
}

struct OrdinalModel {
    heads: [ModelHead; 4],
}

struct MonthFit {
    calibration: Option<HurdleCalibration>,
    effective: EffectiveSupport,
    masks: MonthMasks,
    model: Option<OrdinalModel>,
    nesting_fallback: Option<bool>,
    proposed_rules: Option<Vec<EventRule>>,
    reason: &'static str,
    supported: bool,
    training_support: Support,
    calibration_support: Support,
    uniform_rules: Option<Vec<EventRule>>,
}

struct GridProjection {
    amount: Vec<f64>,
    probabilities: Vec<[f64; 3]>,
}

/// Run one exact future-only rain fit and return sorted LF-terminated JSON bytes.
pub fn run_native_rain_fit_document(
    input: JsonValue,
) -> Result<Vec<u8>, NativeMaintenanceRainFitError> {
    let wire = NativeWirePrimitives;
    let output = match text(&input, "contractVersion").as_deref() {
        Some(FIT_INPUT_VERSION) => {
            let request = RainFitRequest::parse(&input, &wire)?;
            let xgboost = XgBoost::load()?;
            fit_monthly_rain(&request, &xgboost)?
        }
        Some(CONTROL_INPUT_VERSION) => {
            let xgboost = XgBoost::load()?;
            fit_control_reference(&input, &xgboost, &wire)?
        }
        _ => return Err(NativeMaintenanceRainFitError::Contract),
    };
    let bytes = encode_python_sorted_json(&output)?;
    // retain the predecessor's separately bounded public graph.
    if bytes.len() > MAXIMUM_OUTPUT_BYTES {
        return Err(NativeMaintenanceRainFitError::Output);
    }
    Ok(bytes)
}

struct RainFitRequest {
    development_population: JsonValue,
    development_rows: Vec<RainRow>,
    feature_names: JsonValue,
    month: String,
    rows: Vec<RainRow>,
}

impl RainFitRequest {
    /// Admit the exact V3 request and merge immutable duplicate rows.
    fn parse(
        input: &JsonValue,
        wire: &NativeWirePrimitives,
    ) -> Result<Self, NativeMaintenanceRainFitError> {
        if !exact_keys(
            input,
            &[
                "contractVersion",
                "month",
                "featureNames",
                "trainingRows",
                "developmentRows",
                "developmentPopulation",
            ],
        ) || text(input, "contractVersion").as_deref() != Some(FIT_INPUT_VERSION)
        {
            return Err(NativeMaintenanceRainFitError::Contract);
        }
        let month = text(input, "month").ok_or(NativeMaintenanceRainFitError::Input)?;
        let masks = month_masks(&month)?;
        let feature_names = field(input, "featureNames")
            .cloned()
            .ok_or(NativeMaintenanceRainFitError::Input)?;
        validate_feature_names(&feature_names, wire)?;
        let training = array(input, "trainingRows")?;
        let development = array(input, "developmentRows")?;
        if training.len() + development.len() > MAXIMUM_ROWS {
            return Err(NativeMaintenanceRainFitError::Input);
        }
        let mut training_rows = parse_rows(training, masks, wire, false)?;
        let development_rows = parse_rows(development, masks, wire, true)?;
        let population = field(input, "developmentPopulation")
            .cloned()
            .ok_or(NativeMaintenanceRainFitError::Population)?;
        validate_population(&population, &development_rows, wire)?;
        let mut positions = HashMap::with_capacity(training_rows.len() + development_rows.len());
        // retain the last development occurrence only after proving immutable equality.
        for (index, row) in training_rows.iter().enumerate() {
            positions.insert(row.key.clone(), index);
        }
        for row in &development_rows {
            if let Some(index) = positions.get(&row.key).copied() {
                let prior = &training_rows[index];
                if prior.features != row.features
                    || prior.actual != row.actual
                    || prior.raw != row.raw
                    || prior.valid_at != row.valid_at
                    || prior.run_initialized_at != row.run_initialized_at
                {
                    return Err(NativeMaintenanceRainFitError::Input);
                }
                training_rows[index] = row.clone();
            } else {
                positions.insert(row.key.clone(), training_rows.len());
                training_rows.push(row.clone());
            }
        }
        training_rows.sort_by(|left, right| {
            left.valid_at
                .cmp(&right.valid_at)
                .then_with(|| left.key.cmp(&right.key))
        });
        Ok(Self {
            development_population: population,
            development_rows,
            feature_names,
            month,
            rows: training_rows,
        })
    }
}

/// Execute the four-head due fit, historical projections, and native policy screens.
fn fit_monthly_rain(
    request: &RainFitRequest,
    xgboost: &XgBoost,
) -> Result<JsonValue, NativeMaintenanceRainFitError> {
    let final_fit = fit_month(&request.rows, &request.month, xgboost)?;
    let population_sha256 = text(&request.development_population, "populationSha256")
        .ok_or(NativeMaintenanceRainFitError::Population)?;
    let mut report = base_report(request, &final_fit, &population_sha256);
    let Some(final_model) = final_fit.model.as_ref().filter(|_| final_fit.supported) else {
        set_field(
            &mut report,
            "reason",
            string("insufficient_native_fit_support"),
        )?;
        return Ok(report);
    };
    // require every real head before evaluating or publishing a candidate.
    if final_model.heads.iter().any(|head| head.booster.is_none()) {
        set_field(
            &mut report,
            "reason",
            string("insufficient_native_fit_support"),
        )?;
        return Ok(report);
    }
    if request.development_rows.is_empty()
        || request
            .development_rows
            .iter()
            .any(|row| row.gauge_count == 0)
    {
        return Ok(report);
    }
    let mut by_key = HashMap::with_capacity(request.rows.len());
    // bind each development key back to the one combined numerical row.
    for (index, row) in request.rows.iter().enumerate() {
        by_key.insert(row.key.as_str(), index);
    }
    let month_labels = request
        .development_rows
        .iter()
        .map(|row| month_from_hour(row.initialized + 8))
        .collect::<Result<Vec<_>, _>>()?;
    let months = month_labels.iter().cloned().collect::<BTreeSet<_>>();
    let count = request.development_rows.len();
    let mut evaluation_rows = request
        .development_rows
        .iter()
        .map(|row| row.evaluation_row.clone())
        .collect::<Vec<_>>();
    let mut projected = GRID.map(|_| GridProjection {
        amount: vec![f64::NAN; count],
        probabilities: vec![[f64::NAN; 3]; count],
    });
    // reconstruct each original historical decision month without future labels.
    for month in months {
        let fit = fit_month(&request.rows, &month, xgboost)?;
        let Some(model) = fit.model.as_ref().filter(|_| fit.supported) else {
            set_field(
                &mut report,
                "reason",
                string("insufficient_development_month"),
            )?;
            return Ok(report);
        };
        let Some(calibration) = fit.calibration.as_ref() else {
            return Err(NativeMaintenanceRainFitError::Model);
        };
        let scales = historical_control_scales(&request.rows, &month)?;
        let indices = evaluation_indices(&request.rows, fit.masks);
        let features = select_features(&request.rows, &indices);
        let (probabilities, amount) = model.predict(xgboost, &features, indices.len())?;
        let raw = indices
            .iter()
            .map(|index| request.rows[*index].raw)
            .collect::<Vec<_>>();
        let valid_months = indices
            .iter()
            .map(|index| local_month(request.rows[*index].hour))
            .collect::<Result<Vec<_>, _>>()?;
        let grid = project_grid(&raw, &probabilities, &amount, calibration, &valid_months)?;
        let lookup = indices
            .iter()
            .enumerate()
            .map(|(position, index)| (*index, position))
            .collect::<HashMap<_, _>>();
        // align every development row by immutable source key.
        for (position, row) in request.development_rows.iter().enumerate() {
            if month_labels[position] != month {
                continue;
            }
            let combined = *by_key
                .get(row.key.as_str())
                .ok_or(NativeMaintenanceRainFitError::Input)?;
            let Some(local) = lookup.get(&combined).copied() else {
                continue;
            };
            // install the original earlier-only control row for the V3 evaluator.
            let evaluation = historical_evaluation_row(
                row,
                grid[0].amount[local],
                grid[0].probabilities[local],
                scales,
            )?;
            evaluation_rows[position] = Some(evaluation);
            for arm in 0..GRID.len() {
                projected[arm].amount[position] = grid[arm].amount[local];
                projected[arm].probabilities[position] = grid[arm].probabilities[local];
            }
        }
    }
    // reject any missing historical alignment instead of raw-filling it.
    if projected.iter().any(|grid| {
        grid.amount.iter().any(|value| !value.is_finite())
            || grid
                .probabilities
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
    }) {
        set_field(&mut report, "reason", string("development_alignment_gap"))?;
        return Ok(report);
    }
    let reports = screen_grid(
        &request.development_rows,
        &evaluation_rows,
        &projected,
        &request.development_population,
    )?;
    let selected = reports
        .iter()
        .filter(|(_, _, _, eligible)| *eligible)
        .min_by(|left, right| left.2.total_cmp(&right.2).then_with(|| left.0.cmp(right.0)))
        .map(|(name, _, _, _)| *name);
    set_field(
        &mut report,
        "gridReports",
        object(
            reports
                .iter()
                .map(|(name, value, _, _)| ((*name).to_owned(), value.clone()))
                .collect(),
        ),
    )?;
    set_field(
        &mut report,
        "selectedId",
        selected.map(string).unwrap_or(JsonValue::Null),
    )?;
    set_field(
        &mut report,
        "reason",
        string(if selected.is_some() {
            "development_gate_passer"
        } else {
            "no_development_gate_passer"
        }),
    )?;
    // publish only the compact numerical trees for a selected candidate.
    if let Some(selected) = selected {
        let artifact = compact_artifact(
            final_model,
            &request.month,
            selected,
            &request.feature_names,
            final_fit
                .calibration
                .as_ref()
                .ok_or(NativeMaintenanceRainFitError::Model)?,
        )?;
        set_field(&mut report, "artifact", artifact)?;
        set_field(&mut report, "state", string("development_candidate"))?;
    }
    Ok(report)
}

/// Build the exact initial V3 report before development selection.
fn base_report(request: &RainFitRequest, fit: &MonthFit, population_sha256: &str) -> JsonValue {
    object(vec![
        ("artifact".to_owned(), JsonValue::Null),
        ("calibrationDays".to_owned(), number(90.0)),
        ("confirmationOpened".to_owned(), JsonValue::Boolean(false)),
        ("contractVersion".to_owned(), string(FIT_OUTPUT_VERSION)),
        (
            "developmentPopulationSha256".to_owned(),
            string(population_sha256),
        ),
        (
            "developmentRows".to_owned(),
            number(request.development_rows.len() as f64),
        ),
        ("dueMonth".to_owned(), string(&request.month)),
        ("embargoDays".to_owned(), number(7.0)),
        ("features".to_owned(), number(FEATURE_COUNT as f64)),
        ("fitState".to_owned(), month_fit_json(fit, &request.month)),
        (
            "grid".to_owned(),
            JsonValue::Array(GRID.iter().map(|value| string(value)).collect()),
        ),
        ("gridReports".to_owned(), object(Vec::new())),
        ("minimumCalibrationDates".to_owned(), number(60.0)),
        ("nthread".to_owned(), number(1.0)),
        ("reason".to_owned(), string("insufficient_development")),
        ("rounds".to_owned(), number(160.0)),
        ("selectedId".to_owned(), JsonValue::Null),
        ("sourceFitterLegacyCalibrationDays".to_owned(), number(90.0)),
        ("state".to_owned(), string("no_candidate")),
    ])
}

/// Fit one active pre-month control reference without development selection.
fn fit_control_reference(
    input: &JsonValue,
    xgboost: &XgBoost,
    wire: &NativeWirePrimitives,
) -> Result<JsonValue, NativeMaintenanceRainFitError> {
    fit_control_reference_at(input, xgboost, wire, None)
}

/// Fit one control reference with an injectable completion clock for source parity.
fn fit_control_reference_at(
    input: &JsonValue,
    xgboost: &XgBoost,
    wire: &NativeWirePrimitives,
    generated_at: Option<i64>,
) -> Result<JsonValue, NativeMaintenanceRainFitError> {
    if !exact_keys(
        input,
        &[
            "contractVersion",
            "featureNames",
            "month",
            "requestedAt",
            "trainingRows",
        ],
    ) || text(input, "contractVersion").as_deref() != Some(CONTROL_INPUT_VERSION)
    {
        return Err(NativeMaintenanceRainFitError::Contract);
    }
    let feature_names = field(input, "featureNames")
        .cloned()
        .ok_or(NativeMaintenanceRainFitError::Input)?;
    validate_feature_names(&feature_names, wire)?;
    let month = text(input, "month").ok_or(NativeMaintenanceRainFitError::Clock)?;
    let masks = month_masks(&month)?;
    let requested = canonical_milliseconds(
        &text(input, "requestedAt").ok_or(NativeMaintenanceRainFitError::Clock)?,
        wire,
    )?;
    let month_start = masks.decision_start * 3_600_000;
    let cutoff = masks.calibration_stop * 3_600_000;
    if requested < cutoff || requested >= month_start {
        return Err(NativeMaintenanceRainFitError::Clock);
    }
    let values = array(input, "trainingRows")?;
    if values.len() > MAXIMUM_ROWS {
        return Err(NativeMaintenanceRainFitError::Input);
    }
    let mut rows = parse_rows(values, masks, wire, false)?;
    rows.sort_by(|left, right| {
        left.valid_at
            .cmp(&right.valid_at)
            .then_with(|| left.key.cmp(&right.key))
    });
    let fit = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= TRAINING_START_HOUR && row.hour < masks.fit_stop).then_some(index)
        })
        .collect::<Vec<_>>();
    let calibration = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= masks.calibration_start && row.hour < masks.calibration_stop)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let legacy_start = masks.calibration_stop - 45 * 24;
    let legacy_fit_stop = legacy_start - 7 * 24;
    let legacy_fit = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= TRAINING_START_HOUR && row.hour < legacy_fit_stop).then_some(index)
        })
        .collect::<Vec<_>>();
    let legacy_calibration = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= legacy_start && row.hour < masks.calibration_stop).then_some(index)
        })
        .collect::<Vec<_>>();
    let training_support = support(&rows, &fit);
    let calibration_support = support(&rows, &calibration);
    let legacy_wet = legacy_fit
        .iter()
        .filter(|index| rows[**index].actual >= 0.1)
        .count();
    let common_supported = training_support.dates >= 180
        && training_support.hours >= 1_000
        && training_support.wet_dates >= 20
        && training_support.wet_hours >= 100
        && calibration_support.dates >= 60
        && calibration_support.hours >= 500
        && calibration_support.wet_dates >= 5
        && calibration_support.wet_hours >= 20
        && legacy_fit.len() >= 1_000
        && legacy_wet >= 100
        && legacy_calibration.len() >= 200;
    let mut report = control_report_skeleton(&month, cutoff, wire)?;
    if common_supported {
        let features = select_features(&rows, &fit);
        let actual = select_numbers(&rows, &fit, |row| row.actual);
        let hours = select_hours(&rows, &fit);
        let model = OrdinalModel::fit(xgboost, &features, &actual, &hours)?;
        if model.heads.iter().any(|head| head.booster.is_none()) {
            set_field(
                &mut report,
                "reason",
                string("insufficient_ordinal_head_support"),
            )?;
        } else {
            complete_control_report(
                &mut report,
                &rows,
                &fit,
                &calibration,
                &legacy_fit,
                &legacy_calibration,
                legacy_wet,
                training_support,
                calibration_support,
                &feature_names,
                &month,
                masks,
                &model,
                xgboost,
                wire,
            )?;
        }
    }
    let generated = match generated_at {
        Some(value) => value,
        None => {
            let value = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| NativeMaintenanceRainFitError::Clock)?
                .as_millis();
            i64::try_from(value).map_err(|_| NativeMaintenanceRainFitError::Clock)?
        }
    };
    if generated < requested || generated >= month_start {
        return Err(NativeMaintenanceRainFitError::Clock);
    }
    set_field(
        &mut report,
        "generatedAt",
        string(
            &wire
                .format_utc_milliseconds(generated)
                .ok_or(NativeMaintenanceRainFitError::Clock)?,
        ),
    )?;
    Ok(report)
}

/// Build the source's unsupported control-report base.
fn control_report_skeleton(
    month: &str,
    cutoff: i64,
    wire: &NativeWirePrimitives,
) -> Result<JsonValue, NativeMaintenanceRainFitError> {
    let stamp = |value| {
        wire.format_utc_milliseconds(value)
            .map(|value| string(&value))
            .ok_or(NativeMaintenanceRainFitError::Clock)
    };
    Ok(object(vec![
        ("calibrationEndAt".to_owned(), stamp(cutoff)?),
        (
            "calibrationStartAt".to_owned(),
            stamp(cutoff - 90 * 86_400_000)?,
        ),
        ("contractVersion".to_owned(), string(CONTROL_OUTPUT_VERSION)),
        ("generatedAt".to_owned(), JsonValue::Null),
        (
            "legacyCalibrationStartAt".to_owned(),
            stamp(cutoff - 45 * 86_400_000)?,
        ),
        ("modelMonth".to_owned(), string(month)),
        ("ordinalArtifact".to_owned(), JsonValue::Null),
        ("parityRows".to_owned(), JsonValue::Array(Vec::new())),
        ("reason".to_owned(), string("insufficient_control_support")),
        ("scales".to_owned(), JsonValue::Null),
        ("state".to_owned(), string("unsupported")),
        ("support".to_owned(), JsonValue::Null),
        ("trainingMaximumValidAt".to_owned(), JsonValue::Null),
    ]))
}

/// Complete one fully supported control report from fitted heads.
#[allow(clippy::too_many_arguments)]
fn complete_control_report(
    report: &mut JsonValue,
    rows: &[RainRow],
    fit: &[usize],
    calibration: &[usize],
    legacy_fit: &[usize],
    legacy_calibration: &[usize],
    legacy_wet: usize,
    training_support: Support,
    calibration_support: Support,
    feature_names: &JsonValue,
    month: &str,
    masks: MonthMasks,
    model: &OrdinalModel,
    xgboost: &XgBoost,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceRainFitError> {
    let calibration_features = select_features(rows, calibration);
    let (probabilities, amount) =
        model.predict(xgboost, &calibration_features, calibration.len())?;
    let actual = select_numbers(rows, calibration, |row| row.actual);
    let raw = select_numbers(rows, calibration, |row| row.raw);
    let hours = select_hours(rows, calibration);
    let proposed = calibrate_events(&actual, &raw, &probabilities, &hours)?;
    let rules = checked_rules(&actual, &raw, &probabilities, &hours, &proposed)?;
    let categories = event_categories(&raw, &probabilities, &rules)?;
    let base = raw
        .iter()
        .zip(amount.iter())
        .map(|(raw, amount)| (0.25 * raw + 0.75 * amount).clamp(0.0, 30.0))
        .collect::<Vec<_>>();
    let balanced = balanced_weights(&hours);
    let scalar = scalar_calibration(&actual, &base, &categories, &balanced)?;
    let same_window = clipped_scale(&actual, &raw, &balanced, 0.1, 3.0)?;
    let recent_weight = recent_weights(&hours, masks.calibration_stop)?;
    let effective = effective_support(&actual, &hours, &recent_weight)?;
    let recent_supported = effective.dates >= 30.0 && effective.wet_dates >= 3.0;
    let recent = if recent_supported {
        clipped_scale(&actual, &raw, &recent_weight, 0.1, 3.0)?
    } else {
        same_window
    };
    let legacy_actual = select_numbers(rows, legacy_calibration, |row| row.actual);
    let legacy_raw = select_numbers(rows, legacy_calibration, |row| row.raw);
    let legacy_hours = select_hours(rows, legacy_calibration);
    let legacy_weight = balanced_weights(&legacy_hours);
    let denominator = dot(&legacy_weight, &legacy_raw);
    let legacy = if denominator <= 1e-12 {
        1.0
    } else {
        (dot(&legacy_weight, &legacy_actual) / denominator).clamp(0.5, 2.0)
    };
    let artifact = control_artifact(model, month, feature_names, &rules, scalar.scale)?;
    let parity = control_parity(model, xgboost, &rules, scalar.scale)?;
    let maximum = fit
        .iter()
        .map(|index| rows[*index].hour)
        .max()
        .ok_or(NativeMaintenanceRainFitError::Model)?
        * 3_600_000;
    set_field(report, "ordinalArtifact", artifact)?;
    set_field(report, "parityRows", JsonValue::Array(parity))?;
    set_field(report, "reason", string("pre_month_reference"))?;
    set_field(
        report,
        "scales",
        object(vec![
            ("legacy".to_owned(), number(legacy)),
            ("recent".to_owned(), number(recent)),
            (
                "recentSupported".to_owned(),
                JsonValue::Boolean(recent_supported),
            ),
            ("sameWindow".to_owned(), number(same_window)),
        ]),
    )?;
    set_field(report, "state", string("supported"))?;
    set_field(
        report,
        "support",
        control_support_json(
            training_support,
            calibration_support,
            effective,
            legacy_fit.len(),
            legacy_wet,
            legacy_calibration.len(),
        ),
    )?;
    set_field(
        report,
        "trainingMaximumValidAt",
        string(
            &wire
                .format_utc_milliseconds(maximum)
                .ok_or(NativeMaintenanceRainFitError::Clock)?,
        ),
    )?;
    Ok(())
}

/// Serialize control-reference support counters.
fn control_support_json(
    training: Support,
    calibration: Support,
    effective: EffectiveSupport,
    legacy_training_rows: usize,
    legacy_training_wet_rows: usize,
    legacy_calibration_rows: usize,
) -> JsonValue {
    object(vec![
        (
            "calibrationDates".to_owned(),
            number(calibration.dates as f64),
        ),
        (
            "calibrationHours".to_owned(),
            number(calibration.hours as f64),
        ),
        (
            "calibrationRows".to_owned(),
            number(calibration.rows as f64),
        ),
        (
            "calibrationWetDates".to_owned(),
            number(calibration.wet_dates as f64),
        ),
        (
            "calibrationWetHours".to_owned(),
            number(calibration.wet_hours as f64),
        ),
        ("effectiveDates".to_owned(), number(effective.dates)),
        ("effectiveWetDates".to_owned(), number(effective.wet_dates)),
        (
            "legacyCalibrationRows".to_owned(),
            number(legacy_calibration_rows as f64),
        ),
        (
            "legacyTrainingRows".to_owned(),
            number(legacy_training_rows as f64),
        ),
        (
            "legacyTrainingWetRows".to_owned(),
            number(legacy_training_wet_rows as f64),
        ),
        ("trainingDates".to_owned(), number(training.dates as f64)),
        ("trainingHours".to_owned(), number(training.hours as f64)),
        ("trainingRows".to_owned(), number(training.rows as f64)),
        (
            "trainingWetDates".to_owned(),
            number(training.wet_dates as f64),
        ),
        (
            "trainingWetHours".to_owned(),
            number(training.wet_hours as f64),
        ),
    ])
}

/// Parse and validate one bounded row collection.
fn parse_rows(
    rows: &[JsonValue],
    masks: MonthMasks,
    wire: &NativeWirePrimitives,
    require_gauge: bool,
) -> Result<Vec<RainRow>, NativeMaintenanceRainFitError> {
    let mut seen = HashSet::with_capacity(rows.len());
    let mut parsed = Vec::with_capacity(rows.len());
    // close every row before any target reaches the learner.
    for value in rows {
        let key = text(value, "key").ok_or(NativeMaintenanceRainFitError::Input)?;
        if !seen.insert(key.clone())
            || text(value, "evidenceClass").as_deref() != Some("development")
        {
            return Err(NativeMaintenanceRainFitError::Input);
        }
        let source_receipt = canonical_hour_or_millisecond(
            &text(value, "sourceReceiptAt").ok_or(NativeMaintenanceRainFitError::Clock)?,
            wire,
        )?;
        let target_receipt = canonical_hour_or_millisecond(
            &text(value, "targetMaxReceiptAt").ok_or(NativeMaintenanceRainFitError::Clock)?,
            wire,
        )?;
        let valid_at = text(value, "validAt").ok_or(NativeMaintenanceRainFitError::Clock)?;
        let valid_ms = canonical_milliseconds(&valid_at, wire)?;
        if source_receipt >= masks.calibration_stop * 3_600_000
            || target_receipt >= masks.calibration_stop * 3_600_000
            || valid_ms >= masks.calibration_stop * 3_600_000
        {
            return Err(NativeMaintenanceRainFitError::Clock);
        }
        let run_initialized_at =
            text(value, "runInitializedAt").ok_or(NativeMaintenanceRainFitError::Clock)?;
        let initialized_ms = canonical_milliseconds(&run_initialized_at, wire)?;
        let features = numbers_or_nulls(value, "features", FEATURE_COUNT)?;
        let actual = finite_nonnegative(value, "actual")?;
        let raw = finite_nonnegative(value, "raw")?;
        let temperature = finite_number(value, "rawTargetHourTemperatureC")?;
        if temperature <= 2.0 {
            return Err(NativeMaintenanceRainFitError::Input);
        }
        let persistence = nullable_nonnegative(value, "persistencePrediction")?;
        let source_row_sha256 =
            text(value, "sourceRowSha256").ok_or(NativeMaintenanceRainFitError::Input)?;
        let target_row_sha256 =
            text(value, "targetRowSha256").ok_or(NativeMaintenanceRainFitError::Input)?;
        if !valid_hash(&source_row_sha256) || !valid_hash(&target_row_sha256) {
            return Err(NativeMaintenanceRainFitError::Input);
        }
        let model_lead_hours = exact_u64(value, "modelLeadHours")?;
        let operational_horizon_hours = exact_u64(value, "operationalHorizonHours")?;
        if !(9..32).contains(&model_lead_hours)
            || operational_horizon_hours != model_lead_hours - 8
            || (valid_ms - initialized_ms) / 3_600_000 != model_lead_hours as i64
            || (valid_ms - initialized_ms) % 3_600_000 != 0
        {
            return Err(NativeMaintenanceRainFitError::Input);
        }
        let gauge_count = match field(value, "gaugeCount") {
            Some(_) => exact_usize(value, "gaugeCount")?,
            None if !require_gauge => 0,
            None => return Err(NativeMaintenanceRainFitError::Input),
        };
        parsed.push(RainRow {
            actual,
            evaluation_row: field(value, "evaluationRow").cloned(),
            features,
            gauge_count,
            hour: valid_ms / 3_600_000,
            initialized: initialized_ms / 3_600_000,
            key,
            operational_horizon_hours,
            persistence,
            raw,
            raw_target_hour_temperature_c: temperature,
            run_initialized_at,
            source_row_sha256,
            target_row_sha256,
            valid_at,
        });
    }
    Ok(parsed)
}

/// Validate the annual source proof and exact development-row population.
fn validate_population(
    population: &JsonValue,
    rows: &[RainRow],
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceRainFitError> {
    let keys = [
        "contractVersion",
        "cycleHours",
        "developmentEndAt",
        "developmentStartAt",
        "eligibleRowCount",
        "excludedColdRowCount",
        "expectedRowCount",
        "missingSourceRowCount",
        "missingTargetRowCount",
        "observedRowCount",
        "operationalHorizonHours",
        "populationMemberRootSha256",
        "populationReceiptRootSha256",
        "populationSha256",
        "sourceModelLeadHours",
        "sourcePopulation",
    ];
    if !exact_keys(population, &keys)
        || text(population, "contractVersion").as_deref() != Some(POPULATION_VERSION)
        || number_array(population, "cycleHours")? != [0, 6, 12, 18]
        || number_array(population, "sourceModelLeadHours")? != (9..32).collect::<Vec<_>>()
        || number_array(population, "operationalHorizonHours")? != (1..24).collect::<Vec<_>>()
    {
        return Err(NativeMaintenanceRainFitError::Population);
    }
    let start = canonical_milliseconds(
        &text(population, "developmentStartAt").ok_or(NativeMaintenanceRainFitError::Population)?,
        wire,
    )?;
    let end = canonical_milliseconds(
        &text(population, "developmentEndAt").ok_or(NativeMaintenanceRainFitError::Population)?,
        wire,
    )?;
    if end <= start || end - start < 365 * 86_400_000 || end - start > 366 * 86_400_000 {
        return Err(NativeMaintenanceRainFitError::Population);
    }
    let members = array(population, "sourcePopulation")?;
    let expected_count = exact_usize(population, "expectedRowCount")?;
    let observed_count = exact_usize(population, "observedRowCount")?;
    let eligible_count = exact_usize(population, "eligibleRowCount")?;
    let excluded_count = exact_usize(population, "excludedColdRowCount")?;
    if exact_usize(population, "missingSourceRowCount")? != 0
        || exact_usize(population, "missingTargetRowCount")? != 0
        || expected_count != members.len()
        || observed_count != members.len()
    {
        return Err(NativeMaintenanceRainFitError::Population);
    }
    let mut seen = HashSet::with_capacity(members.len());
    let mut eligible = Vec::with_capacity(rows.len());
    let mut member_hashes = Vec::with_capacity(members.len());
    let mut receipt_hashes = BTreeSet::new();
    let mut excluded = 0_usize;
    // bind every value-blind member and its source custody hashes.
    for member in members {
        if !exact_keys(
            member,
            &[
                "issuedAt",
                "key",
                "modelLeadHours",
                "operationalHorizonHours",
                "phaseEligible",
                "sourceMemberSha256",
                "sourceReceiptSha256",
                "targetAvailable",
                "validAt",
            ],
        ) {
            return Err(NativeMaintenanceRainFitError::Population);
        }
        let key = text(member, "key").ok_or(NativeMaintenanceRainFitError::Population)?;
        let source =
            text(member, "sourceMemberSha256").ok_or(NativeMaintenanceRainFitError::Population)?;
        let receipt =
            text(member, "sourceReceiptSha256").ok_or(NativeMaintenanceRainFitError::Population)?;
        if !seen.insert(key.clone()) || !valid_hash(&source) || !valid_hash(&receipt) {
            return Err(NativeMaintenanceRainFitError::Population);
        }
        let phase = boolean(member, "phaseEligible")?;
        let target = boolean(member, "targetAvailable")?;
        let lead = exact_u64(member, "modelLeadHours")?;
        let horizon = exact_u64(member, "operationalHorizonHours")?;
        if !(9..32).contains(&lead) || horizon != lead - 8 {
            return Err(NativeMaintenanceRainFitError::Population);
        }
        let issued = canonical_milliseconds(
            &text(member, "issuedAt").ok_or(NativeMaintenanceRainFitError::Population)?,
            wire,
        )?;
        let valid = canonical_milliseconds(
            &text(member, "validAt").ok_or(NativeMaintenanceRainFitError::Population)?,
            wire,
        )?;
        if valid < start || valid >= end || issued != valid - (lead as i64 - 8) * 3_600_000 {
            return Err(NativeMaintenanceRainFitError::Population);
        }
        if phase {
            if !target {
                return Err(NativeMaintenanceRainFitError::Population);
            }
            eligible.push(key);
        } else {
            excluded += 1;
        }
        member_hashes.push(source);
        receipt_hashes.insert(receipt);
    }
    member_hashes.sort();
    eligible.sort();
    let mut row_keys = rows.iter().map(|row| row.key.clone()).collect::<Vec<_>>();
    row_keys.sort();
    let member_root = hash_json(&JsonValue::Array(
        member_hashes.iter().map(|value| string(value)).collect(),
    ))?;
    let receipt_root = hash_json(&JsonValue::Array(
        receipt_hashes.iter().map(|value| string(value)).collect(),
    ))?;
    let population_root = hash_json(&JsonValue::Array(members.to_vec()))?;
    if eligible_count != eligible.len()
        || excluded_count != excluded
        || eligible != row_keys
        || text(population, "populationMemberRootSha256").as_deref() != Some(member_root.as_str())
        || text(population, "populationReceiptRootSha256").as_deref() != Some(receipt_root.as_str())
        || text(population, "populationSha256").as_deref() != Some(population_root.as_str())
    {
        return Err(NativeMaintenanceRainFitError::Population);
    }
    Ok(())
}

/// Validate feature order against the retained reviewed rain runtime.
fn validate_feature_names(
    feature_names: &JsonValue,
    wire: &NativeWirePrimitives,
) -> Result<(), NativeMaintenanceRainFitError> {
    let retained = wire
        .decode_json(RETAINED_RAIN_ARTIFACT)
        .ok_or(NativeMaintenanceRainFitError::Codec)?;
    if field(&retained, "featureNames") != Some(feature_names)
        || !matches!(feature_names, JsonValue::Array(values) if values.len() == FEATURE_COUNT)
    {
        return Err(NativeMaintenanceRainFitError::Contract);
    }
    Ok(())
}

/// Build the source's ninety-day calibration and due-month masks.
fn month_masks(month: &str) -> Result<MonthMasks, NativeMaintenanceRainFitError> {
    let (year, number) = parse_month(month)?;
    let start = month_start_hour(year, number);
    let (next_year, next_month) = if number == 12 {
        (year + 1, 1)
    } else {
        (year, number + 1)
    };
    let calibration_stop = start - 7 * DAY_HOURS;
    let calibration_start = calibration_stop - 90 * DAY_HOURS;
    Ok(MonthMasks {
        calibration_start,
        calibration_stop,
        decision_start: start,
        decision_stop: month_start_hour(next_year, next_month),
        fit_stop: calibration_start - 7 * DAY_HOURS,
    })
}

/// Parse one exact ASCII year-month label.
fn parse_month(month: &str) -> Result<(i32, u8), NativeMaintenanceRainFitError> {
    let bytes = month.as_bytes();
    if bytes.len() != 7
        || bytes[4] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || byte.is_ascii_digit())
    {
        return Err(NativeMaintenanceRainFitError::Clock);
    }
    let year = month[..4]
        .parse::<i32>()
        .map_err(|_| NativeMaintenanceRainFitError::Clock)?;
    let number = month[5..]
        .parse::<u8>()
        .map_err(|_| NativeMaintenanceRainFitError::Clock)?;
    if year < 1970 || !(1..=12).contains(&number) {
        return Err(NativeMaintenanceRainFitError::Clock);
    }
    Ok((year, number))
}

/// Convert one civil month start to Unix hours without timezone normalization.
fn month_start_hour(year: i32, month: u8) -> i64 {
    let adjusted_year = year - i32::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = i32::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    i64::from(era * 146_097 + day_of_era - 719_468) * 24
}

/// Fit one monthly four-head model with the frozen support and calibration policy.
fn fit_month(
    rows: &[RainRow],
    month: &str,
    xgboost: &XgBoost,
) -> Result<MonthFit, NativeMaintenanceRainFitError> {
    let masks = month_masks(month)?;
    let fit = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= TRAINING_START_HOUR && row.hour < masks.fit_stop).then_some(index)
        })
        .collect::<Vec<_>>();
    let calibration = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= masks.calibration_start && row.hour < masks.calibration_stop)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let training_support = support(rows, &fit);
    let calibration_support = support(rows, &calibration);
    let actual_calibration = select_numbers(rows, &calibration, |row| row.actual);
    let hours_calibration = select_hours(rows, &calibration);
    let recent_weight = recent_weights(&hours_calibration, masks.calibration_stop)?;
    let effective = effective_support(&actual_calibration, &hours_calibration, &recent_weight)?;
    let supported = training_support.dates >= 180
        && training_support.hours >= 1_000
        && training_support.wet_dates >= 20
        && training_support.wet_hours >= 100
        && calibration_support.dates >= 60
        && calibration_support.hours >= 500
        && calibration_support.wet_dates >= 5
        && calibration_support.wet_hours >= 20
        && effective.dates >= 30.0
        && effective.wet_dates >= 3.0;
    if !supported {
        return Ok(MonthFit {
            calibration: None,
            effective,
            masks,
            model: None,
            nesting_fallback: None,
            proposed_rules: None,
            reason: "insufficient_support",
            supported: false,
            training_support,
            calibration_support,
            uniform_rules: None,
        });
    }
    let features = select_features(rows, &fit);
    let actual = select_numbers(rows, &fit, |row| row.actual);
    let hours = select_hours(rows, &fit);
    let model = OrdinalModel::fit(xgboost, &features, &actual, &hours)?;
    let calibration_features = select_features(rows, &calibration);
    let (probabilities, amount) =
        model.predict(xgboost, &calibration_features, calibration.len())?;
    let raw = select_numbers(rows, &calibration, |row| row.raw);
    let proposed_rules = calibrate_events(
        &actual_calibration,
        &raw,
        &probabilities,
        &hours_calibration,
    )?;
    let uniform_rules = checked_rules(
        &actual_calibration,
        &raw,
        &probabilities,
        &hours_calibration,
        &proposed_rules,
    )?;
    let nesting_fallback = uniform_rules
        .iter()
        .any(|rule| rule.reason == "nesting_safety_fallback");
    let base = raw
        .iter()
        .zip(amount.iter())
        .map(|(raw, amount)| (0.25 * raw + 0.75 * amount).clamp(0.0, 30.0))
        .collect::<Vec<_>>();
    let hurdle = calibrate_hurdle(
        &actual_calibration,
        &hours_calibration,
        &raw,
        &probabilities,
        &base,
        &uniform_rules,
        masks.calibration_stop,
    )?;
    Ok(MonthFit {
        calibration: Some(hurdle),
        effective,
        masks,
        model: Some(model),
        nesting_fallback: Some(nesting_fallback),
        proposed_rules: Some(proposed_rules),
        reason: "wind_vector_hurdle_calibrated",
        supported: true,
        training_support,
        calibration_support,
        uniform_rules: Some(uniform_rules),
    })
}

impl OrdinalModel {
    /// Fit the exact three logistic heads and one conditional gamma head.
    fn fit(
        xgboost: &XgBoost,
        features: &[f32],
        actual: &[f64],
        hours: &[i64],
    ) -> Result<Self, NativeMaintenanceRainFitError> {
        if actual.is_empty()
            || actual.len() != hours.len()
            || features.len() != actual.len() * FEATURE_COUNT
        {
            return Err(NativeMaintenanceRainFitError::Numeric);
        }
        let balanced = balanced_weights(hours);
        let heads = std::array::from_fn(|index| {
            // count the amount head on its wet-only label population.
            let threshold = if index == 3 {
                THRESHOLDS[0]
            } else {
                THRESHOLDS[index]
            };
            let support = head_support(actual, hours, threshold);
            ModelHead {
                booster: None,
                bytes: None,
                objective: if index == 3 {
                    "reg:gamma"
                } else {
                    "binary:logistic"
                },
                reason: if index == 3 {
                    "insufficient_wet_support"
                } else {
                    "insufficient_positive_support"
                },
                support,
            }
        });
        let mut model = Self { heads };
        // fit every supported event head with the identical full training matrix.
        for (index, threshold) in THRESHOLDS.iter().copied().enumerate() {
            let support = model.heads[index].support;
            if support.positive_hours < 10 || support.positive_dates < 3 {
                continue;
            }
            let labels = actual
                .iter()
                .map(|value| if *value >= threshold { 1.0 } else { 0.0 })
                .collect::<Vec<_>>();
            let weights = balanced
                .iter()
                .map(|value| *value * actual.len() as f64)
                .collect::<Vec<_>>();
            let booster =
                xgboost.train(features, actual.len(), &labels, &weights, "binary:logistic")?;
            let bytes = booster.save_json(xgboost)?;
            model.heads[index].booster = Some(booster);
            model.heads[index].bytes = Some(bytes);
            model.heads[index].reason = "fitted";
        }
        let wet = actual
            .iter()
            .enumerate()
            .filter_map(|(index, value)| (*value >= 0.1).then_some(index))
            .collect::<Vec<_>>();
        let support = model.heads[3].support;
        if support.positive_hours >= 100 && support.positive_dates >= 20 {
            let mut wet_features = Vec::with_capacity(wet.len() * FEATURE_COUNT);
            let mut labels = Vec::with_capacity(wet.len());
            let wet_hours = wet.iter().map(|index| hours[*index]).collect::<Vec<_>>();
            let wet_weights = balanced_weights(&wet_hours);
            // preserve conditional wet row order and f32 feature bytes.
            for index in &wet {
                wet_features.extend_from_slice(
                    &features[index * FEATURE_COUNT..(index + 1) * FEATURE_COUNT],
                );
                labels.push(actual[*index]);
            }
            let weights = wet_weights
                .iter()
                .map(|value| *value * wet.len() as f64)
                .collect::<Vec<_>>();
            let booster =
                xgboost.train(&wet_features, wet.len(), &labels, &weights, "reg:gamma")?;
            let bytes = booster.save_json(xgboost)?;
            model.heads[3].booster = Some(booster);
            model.heads[3].bytes = Some(bytes);
            model.heads[3].reason = "fitted";
        }
        Ok(model)
    }

    /// Predict all supported heads while retaining NaN for absent source heads.
    fn predict(
        &self,
        xgboost: &XgBoost,
        features: &[f32],
        rows: usize,
    ) -> Result<(Vec<[f64; 3]>, Vec<f64>), NativeMaintenanceRainFitError> {
        if features.len() != rows * FEATURE_COUNT {
            return Err(NativeMaintenanceRainFitError::Numeric);
        }
        let mut probabilities = vec![[f64::NAN; 3]; rows];
        // preserve each occurrence head independently.
        for (index, head) in self.heads.iter().take(3).enumerate() {
            if let Some(booster) = &head.booster {
                let values = booster.predict(xgboost, features, rows)?;
                for (row, value) in values.into_iter().enumerate() {
                    probabilities[row][index] = f64::from(value).clamp(0.0, 1.0);
                }
            }
        }
        let mut amount = vec![f64::NAN; rows];
        if let Some(booster) = &self.heads[3].booster {
            let values = booster.predict(xgboost, features, rows)?;
            // clip conditional gamma predictions to the physical source range.
            for (target, value) in amount.iter_mut().zip(values) {
                *target = f64::from(value).clamp(0.1, 30.0);
            }
        }
        Ok((probabilities, amount))
    }
}

/// Project the fixed seven-arm grid from one fitted month.
fn project_grid(
    raw: &[f64],
    probabilities: &[[f64; 3]],
    amount: &[f64],
    calibration: &HurdleCalibration,
    months: &[u8],
) -> Result<[GridProjection; 7], NativeMaintenanceRainFitError> {
    if raw.len() != probabilities.len() || raw.len() != amount.len() || raw.len() != months.len() {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    let mut output = std::array::from_fn(|_| GridProjection {
        amount: Vec::with_capacity(raw.len()),
        probabilities: Vec::with_capacity(raw.len()),
    });
    // apply only the seven prespecified projection arms.
    for (arm, projection) in output.iter_mut().enumerate() {
        let mut adjusted = probabilities.to_vec();
        // modify only the wet-event logit in its selected season.
        if arm == 3 || arm == 4 {
            for (index, probability) in adjusted.iter_mut().enumerate() {
                let selected = if arm == 3 {
                    (3..=5).contains(&months[index])
                } else {
                    (6..=8).contains(&months[index])
                };
                if selected {
                    let value = probability[0].clamp(1e-12, 1.0 - 1e-12);
                    probability[0] = 1.0 / (1.0 + (-(value / (1.0 - value)).ln() - 0.20).exp());
                }
            }
        }
        // enforce cumulative minimum nesting only in its own arm.
        if arm == 5 {
            for probability in &mut adjusted {
                probability[1] = probability[1].min(probability[0]);
                probability[2] = probability[2].min(probability[1]);
            }
        }
        let base = raw
            .iter()
            .zip(amount.iter())
            .map(|(raw, amount)| (0.25 * raw + 0.75 * amount).clamp(0.0, 30.0))
            .collect::<Vec<_>>();
        let mut predicted = hurdle_predict(raw, &adjusted, &base, calibration)?;
        // apply the two winter scalar arms after hurdle calibration.
        if arm == 1 || arm == 2 {
            for (index, value) in predicted.iter_mut().enumerate() {
                if matches!(months[index], 12 | 1 | 2) {
                    *value *= if arm == 1 { 0.90 } else { 0.95 };
                }
            }
        }
        // blend raw only for heavy events in the final fixed arm.
        if arm == 6 {
            for (index, value) in predicted.iter_mut().enumerate() {
                if raw[index] >= 1.0 {
                    *value = 0.25 * raw[index] + 0.75 * *value;
                }
            }
        }
        // preserve raw wet and heavy event calls after every arm.
        for (index, value) in predicted.iter_mut().enumerate() {
            *value = if raw[index] >= 1.0 {
                raw[index]
            } else if raw[index] >= 0.1 {
                value.max(0.1)
            } else {
                *value
            };
            if !value.is_finite() || *value < 0.0 {
                return Err(NativeMaintenanceRainFitError::Numeric);
            }
        }
        projection.amount = predicted;
        projection.probabilities = adjusted;
    }
    Ok(output)
}

/// Calibrate the three uniform occurrence rules on date-balanced rows.
fn calibrate_events(
    actual: &[f64],
    raw: &[f64],
    probabilities: &[[f64; 3]],
    hours: &[i64],
) -> Result<Vec<EventRule>, NativeMaintenanceRainFitError> {
    if actual.len() != raw.len()
        || actual.len() != probabilities.len()
        || actual.len() != hours.len()
    {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    let weight = balanced_weights(hours);
    let minimum_hours = [10, 5, 3];
    let mut rules = Vec::with_capacity(3);
    // calibrate each physical event independently before nested projection.
    for index in 0..3 {
        let threshold = THRESHOLDS[index];
        let support = head_support(actual, hours, threshold);
        let observed = actual
            .iter()
            .map(|value| *value >= threshold)
            .collect::<Vec<_>>();
        let raw_called = raw
            .iter()
            .map(|value| *value >= threshold)
            .collect::<Vec<_>>();
        let raw_metrics = event_metrics(&observed, &raw_called, &weight);
        let mut rule = EventRule {
            candidate: None,
            cutoff: None,
            head: HEAD_NAMES[index],
            raw: raw_metrics,
            reason: "insufficient_calibration_support",
            support,
            target_recall: None,
            threshold,
        };
        if support.positive_hours >= minimum_hours[index] && support.positive_dates >= 2 {
            let scores = probabilities
                .iter()
                .map(|value| value[index])
                .collect::<Vec<_>>();
            if scores.iter().all(|value| value.is_nan()) {
                rule.reason = "missing_model_head";
            } else {
                if scores
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                {
                    return Err(NativeMaintenanceRainFitError::Numeric);
                }
                let target = (raw_metrics.pod + 0.05).min(1.0);
                rule.target_recall = Some(target);
                let mut choices = scores
                    .iter()
                    .zip(observed.iter())
                    .filter_map(|(score, selected)| selected.then_some(*score))
                    .collect::<Vec<_>>();
                choices.sort_by(|left, right| right.total_cmp(left));
                choices.dedup_by(|left, right| left.total_cmp(right).is_eq());
                // retain the first descending tied positive score meeting recall.
                for score in choices {
                    let called = scores
                        .iter()
                        .map(|value| *value >= score)
                        .collect::<Vec<_>>();
                    let candidate = event_metrics(&observed, &called, &weight);
                    if candidate.pod + 1e-12 >= target {
                        rule.cutoff = Some(score);
                        rule.candidate = Some(candidate);
                        break;
                    }
                }
                if let Some(candidate) = rule.candidate {
                    if candidate.far <= raw_metrics.far + 0.05 + 1e-12
                        && candidate.csi + 1e-12 >= raw_metrics.csi - 0.01
                    {
                        rule.reason = "calibrated";
                    } else {
                        rule.cutoff = None;
                        rule.reason = "calibration_safety_regression";
                    }
                } else {
                    rule.reason = "calibration_safety_regression";
                }
            }
        }
        rules.push(rule);
    }
    Ok(rules)
}

/// Apply the source composite nesting guard to uniform rules.
fn checked_rules(
    actual: &[f64],
    raw: &[f64],
    probabilities: &[[f64; 3]],
    hours: &[i64],
    rules: &[EventRule],
) -> Result<Vec<EventRule>, NativeMaintenanceRainFitError> {
    let categories = event_categories(raw, probabilities, rules)?;
    let proposed = project_amount(&vec![1.0; raw.len()], &categories, 1.0)?;
    let weight = balanced_weights(hours);
    let mut safe = true;
    // preserve all three raw event safety comparisons.
    for threshold in THRESHOLDS {
        let observed = actual
            .iter()
            .map(|value| *value >= threshold)
            .collect::<Vec<_>>();
        let baseline = raw
            .iter()
            .map(|value| *value >= threshold)
            .collect::<Vec<_>>();
        let called = proposed
            .iter()
            .map(|value| *value >= threshold)
            .collect::<Vec<_>>();
        let raw_metrics = event_metrics(&observed, &baseline, &weight);
        let candidate = event_metrics(&observed, &called, &weight);
        if candidate.pod + 1e-12 < raw_metrics.pod
            || candidate.far > raw_metrics.far + 0.05 + 1e-12
            || candidate.csi + 1e-12 < raw_metrics.csi - 0.01
        {
            safe = false;
        }
    }
    if safe {
        return Ok(rules.to_vec());
    }
    Ok(rules
        .iter()
        .cloned()
        .map(|mut rule| {
            rule.cutoff = None;
            rule.reason = "nesting_safety_fallback";
            rule
        })
        .collect())
}

/// Fit the recent wet cutoff plus global and category amount scalars.
fn calibrate_hurdle(
    actual: &[f64],
    hours: &[i64],
    raw: &[f64],
    probabilities: &[[f64; 3]],
    base: &[f64],
    uniform_rules: &[EventRule],
    stop: i64,
) -> Result<HurdleCalibration, NativeMaintenanceRainFitError> {
    let balanced = balanced_weights(hours);
    let observed = actual.iter().map(|value| *value >= 0.1).collect::<Vec<_>>();
    let positive_hours = hours
        .iter()
        .zip(observed.iter())
        .filter_map(|(hour, selected)| selected.then_some(*hour))
        .collect::<BTreeSet<_>>();
    let positive_dates = positive_hours
        .iter()
        .map(|hour| hour.div_euclid(24))
        .collect::<BTreeSet<_>>();
    let raw_rules = uniform_rules
        .iter()
        .cloned()
        .map(|mut rule| {
            rule.cutoff = None;
            rule
        })
        .collect::<Vec<_>>();
    let raw_categories = event_categories(raw, probabilities, &raw_rules)?;
    let old_categories = event_categories(raw, probabilities, uniform_rules)?;
    let raw_called = raw_categories
        .iter()
        .map(|value| *value >= 1)
        .collect::<Vec<_>>();
    let old_called = old_categories
        .iter()
        .map(|value| *value >= 1)
        .collect::<Vec<_>>();
    let raw_metrics = event_metrics(&observed, &raw_called, &balanced);
    let old_metrics = event_metrics(&observed, &old_called, &balanced);
    let mut rules = uniform_rules.to_vec();
    let mut best_metrics = old_metrics;
    let mut best_cutoff = rules[0].cutoff;
    let mut best_rank = (old_metrics.pod + 1e-12 >= raw_metrics.pod
        && old_metrics.far <= raw_metrics.far + 0.05 + 1e-12)
        .then_some((old_metrics.csi, best_cutoff.unwrap_or(f64::NEG_INFINITY)));
    let mut candidate_scores = 0_usize;
    let mut feasible_scores = 0_usize;
    let mut optimized = false;
    if positive_hours.len() >= 10 && positive_dates.len() >= 2 {
        let scores = probabilities
            .iter()
            .map(|value| value[0])
            .collect::<Vec<_>>();
        if !scores.iter().all(|value| value.is_nan()) {
            let mut choices = scores;
            choices.sort_by(f64::total_cmp);
            choices.dedup_by(|left, right| left.total_cmp(right).is_eq());
            candidate_scores = choices.len();
            // compare every native wet score by final nested-call CSI.
            for score in choices {
                let mut candidate_rules = uniform_rules.to_vec();
                candidate_rules[0].cutoff = Some(score);
                let categories = event_categories(raw, probabilities, &candidate_rules)?;
                let called = categories
                    .iter()
                    .map(|value| *value >= 1)
                    .collect::<Vec<_>>();
                let metrics = event_metrics(&observed, &called, &balanced);
                if metrics.pod + 1e-12 < raw_metrics.pod
                    || metrics.far > raw_metrics.far + 0.05 + 1e-12
                {
                    continue;
                }
                feasible_scores += 1;
                let rank = (metrics.csi, score);
                if best_rank.is_none_or(|best| rank > best) {
                    best_rank = Some(rank);
                    best_metrics = metrics;
                    best_cutoff = Some(score);
                    rules = candidate_rules;
                    optimized = true;
                }
            }
        }
    }
    let reason = if positive_hours.len() < 10 || positive_dates.len() < 2 {
        "insufficient_wet_support"
    } else if probabilities.iter().all(|value| value[0].is_nan()) {
        "missing_wet_head"
    } else if optimized {
        "optimized_nested_csi"
    } else if feasible_scores == 0 {
        "no_feasible_new_cutoff"
    } else {
        "old_wet_rule_best"
    };
    let categories = event_categories(raw, probabilities, &rules)?;
    let recent = recent_weights(hours, stop)?;
    let global = scalar_calibration(actual, base, &categories, &recent)?;
    let categories_state = std::array::from_fn(|offset| {
        let category = (offset + 1) as u8;
        let selected = categories
            .iter()
            .enumerate()
            .filter_map(|(index, value)| (*value == category).then_some(index))
            .collect::<Vec<_>>();
        let wet = selected
            .iter()
            .copied()
            .filter(|index| actual[*index] >= 0.1)
            .collect::<Vec<_>>();
        let wet_hours = wet
            .iter()
            .map(|index| hours[*index])
            .collect::<BTreeSet<_>>();
        let wet_dates = wet_hours
            .iter()
            .map(|hour| hour.div_euclid(24))
            .collect::<BTreeSet<_>>();
        let predicted_hours = selected
            .iter()
            .map(|index| hours[*index])
            .collect::<BTreeSet<_>>();
        let predicted_dates = predicted_hours
            .iter()
            .map(|hour| hour.div_euclid(24))
            .collect::<BTreeSet<_>>();
        CategoryCalibration {
            calibration: None,
            observed_dry_rows: selected
                .iter()
                .filter(|index| actual[**index] < 0.1)
                .count(),
            observed_wet_dates: wet_dates.len(),
            observed_wet_hours: wet_hours.len(),
            observed_wet_rows: wet.len(),
            predicted_dates: predicted_dates.len(),
            predicted_hours: predicted_hours.len(),
            reason: "global_recent_fallback_insufficient_wet_support",
            rows: selected.len(),
            scale: global.scale,
            supported: false,
        }
    });
    let mut categories_state = categories_state;
    // replace the global scalar only for independently supported categories.
    for (offset, state) in categories_state.iter_mut().enumerate() {
        if state.observed_wet_hours < 20 || state.observed_wet_dates < 5 {
            continue;
        }
        let category = (offset + 1) as u8;
        let selected = categories
            .iter()
            .enumerate()
            .filter_map(|(index, value)| (*value == category).then_some(index))
            .collect::<Vec<_>>();
        // normalize the selected category with NumPy's pairwise sum order.
        let selected_mass = selected
            .iter()
            .map(|index| recent[*index])
            .collect::<Vec<_>>();
        let mass = numpy_sum(&selected_mass);
        let weights = selected_mass
            .iter()
            .map(|value| *value / mass)
            .collect::<Vec<_>>();
        let selected_actual = selected
            .iter()
            .map(|index| actual[*index])
            .collect::<Vec<_>>();
        let selected_base = selected
            .iter()
            .map(|index| base[*index])
            .collect::<Vec<_>>();
        let selected_categories = vec![category; selected.len()];
        let scalar = scalar_calibration(
            &selected_actual,
            &selected_base,
            &selected_categories,
            &weights,
        )?;
        state.scale = scalar.scale;
        state.calibration = Some(scalar);
        state.reason = "category_recent_calibrated";
        state.supported = true;
    }
    let effective = effective_support(actual, hours, &recent)?;
    Ok(HurdleCalibration {
        calibration_rows: actual.len(),
        calibration_stop: stop,
        categories: categories_state,
        dates: hours
            .iter()
            .map(|hour| hour.div_euclid(24))
            .collect::<BTreeSet<_>>()
            .len(),
        effective,
        global,
        observed_wet_dates: positive_dates.len(),
        observed_wet_hours: positive_hours.len(),
        observed_wet_rows: observed.iter().filter(|value| **value).count(),
        rows: actual.len(),
        rules,
        uniform_rules: uniform_rules.to_vec(),
        wet_selection: WetSelection {
            candidate_scores,
            feasible_scores,
            old_nested_metrics: old_metrics,
            positive_dates: positive_dates.len(),
            positive_hours: positive_hours.len(),
            raw_metrics,
            reason,
            selected_cutoff: best_cutoff,
            selected_metrics: best_metrics,
        },
    })
}

/// Apply one frozen hurdle calibration without reading labels.
fn hurdle_predict(
    raw: &[f64],
    probabilities: &[[f64; 3]],
    base: &[f64],
    state: &HurdleCalibration,
) -> Result<Vec<f64>, NativeMaintenanceRainFitError> {
    let categories = event_categories(raw, probabilities, &state.rules)?;
    let mut result = vec![0.0; raw.len()];
    // keep category zero exact and scale each called category separately.
    for category in 1..=3_u8 {
        let scale = state.categories[usize::from(category - 1)].scale;
        for index in 0..raw.len() {
            if categories[index] == category {
                result[index] = project_amount_value(base[index], category, scale)?;
            }
        }
    }
    Ok(result)
}

/// Convert independent head calls into the highest nested category.
fn event_categories(
    raw: &[f64],
    probabilities: &[[f64; 3]],
    rules: &[EventRule],
) -> Result<Vec<u8>, NativeMaintenanceRainFitError> {
    if raw.len() != probabilities.len() || rules.len() != 3 {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    let mut categories = vec![0_u8; raw.len()];
    // higher matching thresholds overwrite lower categories.
    for index in 0..3 {
        if rules[index].threshold != THRESHOLDS[index] {
            return Err(NativeMaintenanceRainFitError::Numeric);
        }
        for row in 0..raw.len() {
            let called = if let Some(cutoff) = rules[index].cutoff {
                let score = probabilities[row][index];
                if !score.is_finite() || !(0.0..=1.0).contains(&score) {
                    return Err(NativeMaintenanceRainFitError::Numeric);
                }
                score >= cutoff
            } else {
                raw[row] >= THRESHOLDS[index]
            };
            if called {
                categories[row] = (index + 1) as u8;
            }
        }
    }
    Ok(categories)
}

/// Project bounded amounts into their disjoint physical event category.
fn project_amount(
    base: &[f64],
    categories: &[u8],
    scale: f64,
) -> Result<Vec<f64>, NativeMaintenanceRainFitError> {
    if base.len() != categories.len() || !scale.is_finite() || scale <= 0.0 {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    base.iter()
        .zip(categories)
        .map(|(value, category)| project_amount_value(*value, *category, scale))
        .collect()
}

/// Project one scalar with NumPy's exact next-down category bounds.
fn project_amount_value(
    base: f64,
    category: u8,
    scale: f64,
) -> Result<f64, NativeMaintenanceRainFitError> {
    if !base.is_finite() || base < 0.0 || !scale.is_finite() || scale <= 0.0 {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    let scaled = base * scale;
    Ok(match category {
        0 => 0.0,
        1 => scaled.clamp(0.1, f64::from_bits(1.0_f64.to_bits() - 1)),
        2 => scaled.clamp(1.0, f64::from_bits(2.5_f64.to_bits() - 1)),
        3 => scaled.clamp(2.5, 30.0),
        _ => return Err(NativeMaintenanceRainFitError::Numeric),
    })
}

/// Solve one clipped amount scalar with the frozen recent-weighted bisection.
fn scalar_calibration(
    actual: &[f64],
    base: &[f64],
    categories: &[u8],
    weight: &[f64],
) -> Result<ScalarCalibration, NativeMaintenanceRainFitError> {
    if actual.is_empty()
        || actual.len() != base.len()
        || actual.len() != categories.len()
        || actual.len() != weight.len()
    {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    let target = dot(weight, actual);
    let tolerance = 1e-10 + 1e-8 * target.abs();
    let mean = |scale: f64| -> Result<f64, NativeMaintenanceRainFitError> {
        Ok(dot(weight, &project_amount(base, categories, scale)?))
    };
    let mut lower = 0.1;
    let mut upper = 3.0;
    let low_mean = mean(lower)?;
    let high_mean = mean(upper)?;
    let (scale, status) = if target < low_mean {
        (lower, "saturated_low")
    } else if target > high_mean {
        (upper, "saturated_high")
    } else if (mean(1.0)? - target).abs() <= tolerance {
        (1.0, "matched")
    } else {
        // retain the target-containing monotone bracket for exactly sixty-four steps.
        for _ in 0..64 {
            let midpoint = (lower + upper) / 2.0;
            if mean(midpoint)? < target {
                lower = midpoint;
            } else {
                upper = midpoint;
            }
        }
        let lower_rank = ((mean(lower)? - target).abs(), (lower - 1.0).abs(), lower);
        let upper_rank = ((mean(upper)? - target).abs(), (upper - 1.0).abs(), upper);
        let scale = if lower_rank <= upper_rank {
            lower
        } else {
            upper
        };
        let status = if (mean(scale)? - target).abs() <= tolerance {
            "matched"
        } else {
            "unmatched_numeric"
        };
        (scale, status)
    };
    let achieved = mean(scale)?;
    Ok(ScalarCalibration {
        achieved_mean: achieved,
        high_mean,
        low_mean,
        residual: achieved - target,
        scale,
        status,
        target_mean: target,
        tolerance,
    })
}

/// Recreate equal date/hour/vintage weight exactly.
fn balanced_weights(hours: &[i64]) -> Vec<f64> {
    if hours.is_empty() {
        return Vec::new();
    }
    let mut hour_counts = HashMap::<i64, usize>::new();
    let mut date_hours = HashMap::<i64, BTreeSet<i64>>::new();
    // count vintages per valid hour and unique hours per UTC date.
    for hour in hours {
        *hour_counts.entry(*hour).or_default() += 1;
        date_hours
            .entry(hour.div_euclid(24))
            .or_default()
            .insert(*hour);
    }
    let dates = date_hours.len() as f64;
    hours
        .iter()
        .map(|hour| {
            1.0 / (hour_counts[hour] as f64 * date_hours[&hour.div_euclid(24)].len() as f64 * dates)
        })
        .collect()
}

/// Apply the fixed thirty-day decay to date-balanced ninety-day mass.
fn recent_weights(hours: &[i64], stop: i64) -> Result<Vec<f64>, NativeMaintenanceRainFitError> {
    if hours.is_empty()
        || stop % 24 != 0
        || hours
            .iter()
            .any(|hour| *hour < stop - 90 * 24 || *hour >= stop)
    {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    let mut weight = balanced_weights(hours);
    // decay by integer UTC date age before one normalization.
    for (value, hour) in weight.iter_mut().zip(hours) {
        let age = stop.div_euclid(24) - 1 - hour.div_euclid(24);
        *value *= 2.0_f64.powf(-(age as f64) / 30.0);
    }
    let total = numpy_sum(&weight);
    for value in &mut weight {
        *value /= total;
    }
    Ok(weight)
}

/// Count Kish-effective complete and wet UTC-date mass.
fn effective_support(
    actual: &[f64],
    hours: &[i64],
    weight: &[f64],
) -> Result<EffectiveSupport, NativeMaintenanceRainFitError> {
    if actual.len() != hours.len() || actual.len() != weight.len() || actual.is_empty() {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    let mut date_mass = BTreeMap::<i64, f64>::new();
    let mut wet_mass = BTreeMap::<i64, f64>::new();
    // aggregate normalized row mass onto distinct UTC dates.
    for index in 0..actual.len() {
        *date_mass.entry(hours[index].div_euclid(24)).or_default() += weight[index];
        if actual[index] >= 0.1 {
            *wet_mass.entry(hours[index].div_euclid(24)).or_default() += weight[index];
        }
    }
    let date_values = date_mass.values().copied().collect::<Vec<_>>();
    let dates = 1.0 / dot(&date_values, &date_values);
    let wet_values = wet_mass.values().copied().collect::<Vec<_>>();
    let wet_total = numpy_sum(&wet_values);
    let wet_dates = if wet_total == 0.0 {
        0.0
    } else {
        let normalized = wet_values
            .iter()
            .map(|value| value / wet_total)
            .collect::<Vec<_>>();
        1.0 / dot(&normalized, &normalized)
    };
    Ok(EffectiveSupport { dates, wet_dates })
}

/// Count exact distinct date/hour event support.
fn support(rows: &[RainRow], indices: &[usize]) -> Support {
    let hours = indices
        .iter()
        .map(|index| rows[*index].hour)
        .collect::<BTreeSet<_>>();
    let dates = hours
        .iter()
        .map(|hour| hour.div_euclid(24))
        .collect::<BTreeSet<_>>();
    let wet_hours = indices
        .iter()
        .filter_map(|index| (rows[*index].actual >= 0.1).then_some(rows[*index].hour))
        .collect::<BTreeSet<_>>();
    let wet_dates = wet_hours
        .iter()
        .map(|hour| hour.div_euclid(24))
        .collect::<BTreeSet<_>>();
    Support {
        dates: dates.len(),
        hours: hours.len(),
        rows: indices.len(),
        wet_dates: wet_dates.len(),
        wet_hours: wet_hours.len(),
    }
}

/// Count exact threshold-positive row, hour, and date support.
fn head_support(actual: &[f64], hours: &[i64], threshold: f64) -> HeadSupport {
    let selected = actual
        .iter()
        .zip(hours)
        .filter_map(|(value, hour)| (*value >= threshold).then_some(*hour))
        .collect::<Vec<_>>();
    let unique_hours = selected.iter().copied().collect::<BTreeSet<_>>();
    let unique_dates = unique_hours
        .iter()
        .map(|hour| hour.div_euclid(24))
        .collect::<BTreeSet<_>>();
    HeadSupport {
        positive_dates: unique_dates.len(),
        positive_hours: unique_hours.len(),
        positive_rows: selected.len(),
        rows: actual.len(),
    }
}

/// Score one weighted binary event call.
fn event_metrics(observed: &[bool], called: &[bool], weight: &[f64]) -> EventMetrics {
    let mut hit = Vec::with_capacity(observed.len());
    let mut miss = Vec::with_capacity(observed.len());
    let mut false_alarm = Vec::with_capacity(observed.len());
    // retain source boolean dot products for all contingency cells.
    for index in 0..observed.len() {
        hit.push(if observed[index] && called[index] {
            1.0
        } else {
            0.0
        });
        miss.push(if observed[index] && !called[index] {
            1.0
        } else {
            0.0
        });
        false_alarm.push(if !observed[index] && called[index] {
            1.0
        } else {
            0.0
        });
    }
    let hit = dot(weight, &hit);
    let miss = dot(weight, &miss);
    let false_alarm = dot(weight, &false_alarm);
    EventMetrics {
        csi: divide_or_zero(hit, hit + miss + false_alarm),
        far: divide_or_zero(false_alarm, hit + false_alarm),
        pod: divide_or_zero(hit, hit + miss),
    }
}

/// Divide one metric with the source's explicit zero fallback.
fn divide_or_zero(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

/// Compute one aligned dot product.
fn dot(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() {
        return f64::NAN;
    }
    let Some(dot) = OPENBLAS_DDOT.get_or_init(load_openblas_ddot) else {
        return f64::NAN;
    };
    // the reviewed ILP64 cblas function borrows both complete contiguous vectors.
    unsafe { dot(left.len() as i64, left.as_ptr(), 1, right.as_ptr(), 1) }
}

/// Load the exact existing NumPy OpenBLAS dot ABI already admitted by readiness.
fn load_openblas_ddot() -> Option<Ddot> {
    #[cfg(test)]
    let paths = [SANDBOX_OPENBLAS, HOST_OPENBLAS].as_slice();
    #[cfg(not(test))]
    let paths = [SANDBOX_OPENBLAS].as_slice();
    // prefer the isolated production mount and verify bytes before execution.
    for path in paths {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        if bytes.len() != 24_325_657 || digest(&bytes) != OPENBLAS_SHA256 {
            continue;
        }
        let Ok(path) = CString::new(*path) else {
            continue;
        };
        // the verified library handle is retained for process lifetime.
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            continue;
        }
        let dot_name = c"scipy_cblas_ddot64_";
        let threads_name = c"scipy_openblas_set_num_threads64_";
        // resolve only the two reviewed fixed symbols.
        let (dot, threads) = unsafe {
            let dot = libc::dlsym(handle, dot_name.as_ptr());
            let threads = libc::dlsym(handle, threads_name.as_ptr());
            if dot.is_null() || threads.is_null() {
                libc::dlclose(handle);
                continue;
            }
            (
                std::mem::transmute::<*mut c_void, Ddot>(dot),
                std::mem::transmute::<*mut c_void, SetOpenBlasThreads>(threads),
            )
        };
        // preserve the source's single native numerical worker.
        unsafe { threads(1) };
        return Some(dot);
    }
    None
}

/// Reproduce NumPy's large contiguous reduction order.
fn numpy_sum(values: &[f64]) -> f64 {
    if values.len() < 8 {
        values.iter().sum()
    } else if values.len() <= 128 {
        let mut accumulators = [-0.0_f64; 8];
        let mut index = 0_usize;
        // mirror NumPy's eight-lane small pairwise reduction block.
        while index + 8 <= values.len() {
            for lane in 0..8 {
                accumulators[lane] += values[index + lane];
            }
            index += 8;
        }
        let mut result = ((accumulators[0] + accumulators[1])
            + (accumulators[2] + accumulators[3]))
            + ((accumulators[4] + accumulators[5]) + (accumulators[6] + accumulators[7]));
        // preserve NumPy's scalar tail after its eight-lane reduction.
        while index < values.len() {
            result += values[index];
            index += 1;
        }
        result
    } else {
        let mut split = values.len() / 2;
        split -= split % 8;
        numpy_sum(&values[..split]) + numpy_sum(&values[split..])
    }
}

#[derive(Clone, Copy)]
struct HistoricalScales {
    legacy: f64,
    recent: f64,
    same_window: f64,
}

/// Recreate the three earlier-only raw control scales for one historical month.
fn historical_control_scales(
    rows: &[RainRow],
    month: &str,
) -> Result<HistoricalScales, NativeMaintenanceRainFitError> {
    let masks = month_masks(month)?;
    let fit = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= TRAINING_START_HOUR && row.hour < masks.fit_stop).then_some(index)
        })
        .collect::<Vec<_>>();
    let calibration = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= masks.calibration_start && row.hour < masks.calibration_stop)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let training_support = support(rows, &fit);
    let calibration_support = support(rows, &calibration);
    let legacy_start = masks.calibration_stop - 45 * 24;
    let legacy_fit_stop = legacy_start - 7 * 24;
    let legacy_fit = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= TRAINING_START_HOUR && row.hour < legacy_fit_stop).then_some(index)
        })
        .collect::<Vec<_>>();
    let legacy_calibration = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.hour >= legacy_start && row.hour < masks.calibration_stop).then_some(index)
        })
        .collect::<Vec<_>>();
    let legacy_wet = legacy_fit
        .iter()
        .filter(|index| rows[**index].actual >= 0.1)
        .count();
    if training_support.dates < 180
        || training_support.hours < 1_000
        || training_support.wet_dates < 20
        || training_support.wet_hours < 100
        || calibration_support.dates < 60
        || calibration_support.hours < 500
        || calibration_support.wet_dates < 5
        || calibration_support.wet_hours < 20
        || legacy_fit.len() < 1_000
        || legacy_wet < 100
        || legacy_calibration.len() < 200
    {
        return Err(NativeMaintenanceRainFitError::Model);
    }
    let actual = select_numbers(rows, &calibration, |row| row.actual);
    let raw = select_numbers(rows, &calibration, |row| row.raw);
    let hours = select_hours(rows, &calibration);
    let balanced = balanced_weights(&hours);
    let same_window = clipped_scale(&actual, &raw, &balanced, 0.1, 3.0)?;
    let recent_weight = recent_weights(&hours, masks.calibration_stop)?;
    let effective = effective_support(&actual, &hours, &recent_weight)?;
    let recent = if effective.dates >= 30.0 && effective.wet_dates >= 3.0 {
        clipped_scale(&actual, &raw, &recent_weight, 0.1, 3.0)?
    } else {
        same_window
    };
    let legacy_actual = select_numbers(rows, &legacy_calibration, |row| row.actual);
    let legacy_raw = select_numbers(rows, &legacy_calibration, |row| row.raw);
    let legacy_hours = select_hours(rows, &legacy_calibration);
    let legacy_weight = balanced_weights(&legacy_hours);
    let denominator = dot(&legacy_weight, &legacy_raw);
    let legacy = if denominator <= 1e-12 {
        1.0
    } else {
        (dot(&legacy_weight, &legacy_actual) / denominator).clamp(0.5, 2.0)
    };
    Ok(HistoricalScales {
        legacy,
        recent,
        same_window,
    })
}

/// Solve the source clipped raw scalar curve.
fn clipped_scale(
    actual: &[f64],
    raw: &[f64],
    weight: &[f64],
    mut lower: f64,
    mut upper: f64,
) -> Result<f64, NativeMaintenanceRainFitError> {
    let target = dot(weight, actual);
    let mean = |scale: f64| {
        let projected = raw
            .iter()
            .map(|value| (value * scale).clamp(0.0, 30.0))
            .collect::<Vec<_>>();
        dot(weight, &projected)
    };
    let tolerance = 1e-10 + 1e-8 * target.abs();
    if target < mean(lower) {
        return Ok(lower);
    }
    if target > mean(upper) {
        return Ok(upper);
    }
    if (mean(1.0) - target).abs() <= tolerance {
        return Ok(1.0);
    }
    // retain the monotone bracket for exactly sixty-four iterations.
    for _ in 0..64 {
        let midpoint = (lower + upper) / 2.0;
        if mean(midpoint) < target {
            lower = midpoint;
        } else {
            upper = midpoint;
        }
    }
    Ok(
        if ((mean(lower) - target).abs(), (lower - 1.0).abs(), lower)
            <= ((mean(upper) - target).abs(), (upper - 1.0).abs(), upper)
        {
            lower
        } else {
            upper
        },
    )
}

/// Construct one source-exact historical V3 policy row.
fn historical_evaluation_row(
    row: &RainRow,
    incumbent: f64,
    probability: [f64; 3],
    scales: HistoricalScales,
) -> Result<JsonValue, NativeMaintenanceRainFitError> {
    let native_probability = probability_object([
        f64::from(row.raw >= 0.1),
        f64::from(row.raw >= 1.0),
        f64::from(row.raw >= 2.5),
    ]);
    let incumbent_probability = probability_object(probability);
    Ok(object(vec![
        ("actualBestMatchPrediction".to_owned(), number(row.raw)),
        ("applied".to_owned(), JsonValue::Boolean(true)),
        ("candidatePrediction".to_owned(), number(incumbent)),
        (
            "candidateProbability".to_owned(),
            incumbent_probability.clone(),
        ),
        ("evidenceClass".to_owned(), string("historical_development")),
        ("farmTarget".to_owned(), JsonValue::Null),
        ("firstEdgeCommittedAt".to_owned(), JsonValue::Null),
        (
            "horizonHours".to_owned(),
            number(row.operational_horizon_hours as f64),
        ),
        ("incumbentPrediction".to_owned(), number(incumbent)),
        ("incumbentProbability".to_owned(), incumbent_probability),
        ("key".to_owned(), string(&row.key)),
        ("localDate".to_owned(), string(&local_date(row.hour)?)),
        ("nativeSourcePrediction".to_owned(), number(row.raw)),
        ("nativeSourceProbability".to_owned(), native_probability),
        ("nearestThree".to_owned(), JsonValue::Null),
        (
            "persistencePrediction".to_owned(),
            number(row.persistence.unwrap_or(row.raw)),
        ),
        ("provenanceComplete".to_owned(), JsonValue::Boolean(true)),
        ("providerFamily".to_owned(), JsonValue::Null),
        (
            "rawTargetHourTemperatureC".to_owned(),
            number(row.raw_target_hour_temperature_c),
        ),
        (
            "recentVolumeScalePrediction".to_owned(),
            number((row.raw * scales.recent).min(30.0)),
        ),
        ("runKey".to_owned(), string(&row.run_initialized_at)),
        (
            "sameWindowVolumeScalePrediction".to_owned(),
            number((row.raw * scales.same_window).min(30.0)),
        ),
        ("sourceReceiptAt".to_owned(), JsonValue::Null),
        ("sourceRowSha256".to_owned(), string(&row.source_row_sha256)),
        ("stationKey".to_owned(), JsonValue::Null),
        ("target".to_owned(), number(row.actual)),
        ("targetRowSha256".to_owned(), string(&row.target_row_sha256)),
        ("unchangedOrdinalPrediction".to_owned(), number(incumbent)),
        ("validAt".to_owned(), string(&row.valid_at)),
        (
            "volumeScalePrediction".to_owned(),
            number(row.raw * scales.legacy),
        ),
    ]))
}

/// Screen each arm with the native 49-gate evaluator and gauge strata.
fn screen_grid(
    rows: &[RainRow],
    evaluations: &[Option<JsonValue>],
    projected: &[GridProjection; 7],
    population: &JsonValue,
) -> Result<Vec<(&'static str, JsonValue, f64, bool)>, NativeMaintenanceRainFitError> {
    if rows.len() != evaluations.len() {
        return Err(NativeMaintenanceRainFitError::Policy);
    }
    let calendar =
        NativePredecessorModelCalendar::new().ok_or(NativeMaintenanceRainFitError::Policy)?;
    let hours = rows.iter().map(|row| row.hour).collect::<Vec<_>>();
    let weight = balanced_weights(&hours);
    let actual = rows.iter().map(|row| row.actual).collect::<Vec<_>>();
    let mut reports = Vec::with_capacity(GRID.len());
    // evaluate every arm over one identical frozen population.
    for arm in 0..GRID.len() {
        let mut inputs = Vec::with_capacity(rows.len());
        for ((evaluation, amount), probability) in evaluations
            .iter()
            .zip(&projected[arm].amount)
            .zip(&projected[arm].probabilities)
        {
            let mut value = evaluation
                .clone()
                .ok_or(NativeMaintenanceRainFitError::Policy)?;
            set_field(&mut value, "candidatePrediction", number(*amount))?;
            set_field(
                &mut value,
                "candidateProbability",
                probability_object(*probability),
            )?;
            set_field(&mut value, "applied", JsonValue::Boolean(true))?;
            set_field(
                &mut value,
                "evidenceClass",
                string("historical_development"),
            )?;
            set_field(&mut value, "sourceReceiptAt", JsonValue::Null)?;
            set_field(&mut value, "firstEdgeCommittedAt", JsonValue::Null)?;
            let row = create_native_rain_maintenance_policy_row(&value, &calendar)
                .ok_or(NativeMaintenanceRainFitError::Policy)?;
            inputs.push(row.value);
        }
        let gate = evaluate_native_rain_maintenance_development_v3(&inputs, population)
            .ok_or(NativeMaintenanceRainFitError::Policy)?;
        let mut gauge = Vec::with_capacity(3);
        let mut harm = false;
        for (label, low, high) in [("3", 3, 3), ("4_5", 4, 5), ("6_12", 6, 12)] {
            let selected = rows
                .iter()
                .enumerate()
                .filter_map(|(index, row)| (low..=high).contains(&row.gauge_count).then_some(index))
                .collect::<Vec<_>>();
            let dates = selected
                .iter()
                .map(|index| rows[*index].hour.div_euclid(24))
                .collect::<BTreeSet<_>>()
                .len();
            let (candidate_mae, raw_mae) = if !selected.is_empty() && dates >= 60 {
                let selected_hours = selected
                    .iter()
                    .map(|index| rows[*index].hour)
                    .collect::<Vec<_>>();
                let selected_weight = balanced_weights(&selected_hours);
                let candidate_errors = selected
                    .iter()
                    .map(|index| (projected[arm].amount[*index] - rows[*index].actual).abs())
                    .collect::<Vec<_>>();
                let raw_errors = selected
                    .iter()
                    .map(|index| (rows[*index].raw - rows[*index].actual).abs())
                    .collect::<Vec<_>>();
                let candidate = dot(&selected_weight, &candidate_errors);
                let raw_error = dot(&selected_weight, &raw_errors);
                (Some(candidate), Some(raw_error))
            } else {
                (None, None)
            };
            let stratum_harm = candidate_mae
                .zip(raw_mae)
                .is_some_and(|(candidate, raw)| candidate > raw);
            harm |= stratum_harm;
            gauge.push((
                label.to_owned(),
                object(vec![
                    ("candidateMae".to_owned(), nullable_number(candidate_mae)),
                    ("dates".to_owned(), number(dates as f64)),
                    ("harm".to_owned(), JsonValue::Boolean(stratum_harm)),
                    ("rawMae".to_owned(), nullable_number(raw_mae)),
                    ("rows".to_owned(), number(selected.len() as f64)),
                ]),
            ));
        }
        let errors = projected[arm]
            .amount
            .iter()
            .zip(&actual)
            .map(|(prediction, actual)| (prediction - actual).abs())
            .collect::<Vec<_>>();
        let mae = dot(&weight, &errors);
        let passed = boolean(&gate, "passed")?;
        let eligible = passed && !harm;
        reports.push((
            GRID[arm],
            object(vec![
                ("eligible".to_owned(), JsonValue::Boolean(eligible)),
                ("gateReport".to_owned(), gate),
                ("gaugeSupport".to_owned(), object(gauge)),
                ("mae".to_owned(), number(mae)),
            ]),
            mae,
            eligible,
        ));
    }
    Ok(reports)
}

/// Build one portable V1 control artifact from exact fitted native heads.
fn control_artifact(
    model: &OrdinalModel,
    month: &str,
    feature_names: &JsonValue,
    rules: &[EventRule],
    scale: f64,
) -> Result<JsonValue, NativeMaintenanceRainFitError> {
    let mut heads = Vec::with_capacity(4);
    // compact every supported head without retaining source observations.
    for (name, head) in HEAD_NAMES.iter().zip(&model.heads) {
        heads.push((
            (*name).to_owned(),
            compact_head(
                head.bytes
                    .as_deref()
                    .ok_or(NativeMaintenanceRainFitError::Model)?,
                name,
                feature_names,
            )?,
        ));
    }
    Ok(object(vec![
        (
            "categoryScales".to_owned(),
            JsonValue::Array(vec![number(scale), number(scale), number(scale)]),
        ),
        (
            "contractVersion".to_owned(),
            string("rain-hurdle-wind-runtime/v1"),
        ),
        ("featureNames".to_owned(), feature_names.clone()),
        ("heads".to_owned(), object(heads)),
        ("modelMonth".to_owned(), string(month)),
        (
            "rules".to_owned(),
            JsonValue::Array(
                rules
                    .iter()
                    .map(|rule| {
                        object(vec![
                            ("cutoff".to_owned(), nullable_number(rule.cutoff)),
                            ("threshold".to_owned(), number(rule.threshold)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]))
}

/// Replay the three fixed synthetic rows returned with a control reference.
fn control_parity(
    model: &OrdinalModel,
    xgboost: &XgBoost,
    rules: &[EventRule],
    scale: f64,
) -> Result<Vec<JsonValue>, NativeMaintenanceRainFitError> {
    let raw = [0.5, 2.0, 4.0];
    let mut features = Vec::with_capacity(3 * FEATURE_COUNT);
    // preserve zero, one, and missing synthetic geometries.
    for (row, raw_value) in raw.iter().copied().enumerate() {
        for feature in 0..FEATURE_COUNT {
            features.push(if feature == 5 {
                raw_value as f32
            } else if row == 2 {
                f32::NAN
            } else {
                row as f32
            });
        }
    }
    let (probabilities, amount) = model.predict(xgboost, &features, 3)?;
    let base = raw
        .iter()
        .zip(amount.iter())
        .map(|(raw, amount)| (0.25 * raw + 0.75 * amount).clamp(0.0, 30.0))
        .collect::<Vec<_>>();
    let categories = event_categories(&raw, &probabilities, rules)?;
    let prediction = project_amount(&base, &categories, scale)?;
    Ok((0..3)
        .map(|index| {
            object(vec![
                (
                    "features".to_owned(),
                    JsonValue::Array(
                        features[index * FEATURE_COUNT..(index + 1) * FEATURE_COUNT]
                            .iter()
                            .map(|value| {
                                if value.is_finite() {
                                    number(f64::from(*value))
                                } else {
                                    JsonValue::Null
                                }
                            })
                            .collect(),
                    ),
                ),
                ("prediction".to_owned(), number(prediction[index])),
                (
                    "probabilities".to_owned(),
                    JsonValue::Array(
                        probabilities[index]
                            .iter()
                            .map(|value| number(*value))
                            .collect(),
                    ),
                ),
                ("raw".to_owned(), number(raw[index])),
            ])
        })
        .collect())
}

/// Compact the four native JSON models into the portable runtime artifact.
fn compact_artifact(
    model: &OrdinalModel,
    month: &str,
    projection: &str,
    feature_names: &JsonValue,
    calibration: &HurdleCalibration,
) -> Result<JsonValue, NativeMaintenanceRainFitError> {
    let mut heads = Vec::with_capacity(4);
    // retain only numerical tree arrays from every fitted head.
    for (name, head) in HEAD_NAMES.iter().zip(&model.heads) {
        let bytes = head
            .bytes
            .as_deref()
            .ok_or(NativeMaintenanceRainFitError::Model)?;
        heads.push((
            (*name).to_owned(),
            compact_head(bytes, name, feature_names)?,
        ));
    }
    Ok(object(vec![
        (
            "calibration".to_owned(),
            hurdle_calibration_json(calibration),
        ),
        ("contractVersion".to_owned(), string(ARTIFACT_VERSION)),
        ("featureNames".to_owned(), feature_names.clone()),
        ("heads".to_owned(), object(heads)),
        ("modelMonth".to_owned(), string(month)),
        ("projectionId".to_owned(), string(projection)),
    ]))
}

/// Validate and compact one XGBoost JSON model.
fn compact_head(
    bytes: &[u8],
    name: &str,
    feature_names: &JsonValue,
) -> Result<JsonValue, NativeMaintenanceRainFitError> {
    let wire = NativeWirePrimitives;
    let model = wire
        .decode_json(bytes)
        .ok_or(NativeMaintenanceRainFitError::Model)?;
    let learner = field(&model, "learner").ok_or(NativeMaintenanceRainFitError::Model)?;
    let objective = field(learner, "objective").ok_or(NativeMaintenanceRainFitError::Model)?;
    let objective_name = text(objective, "name").ok_or(NativeMaintenanceRainFitError::Model)?;
    let expected = if name == "amount" {
        "reg:gamma"
    } else {
        "binary:logistic"
    };
    if objective_name != expected || field(learner, "feature_names") != Some(feature_names) {
        return Err(NativeMaintenanceRainFitError::Model);
    }
    let gradient =
        field(learner, "gradient_booster").ok_or(NativeMaintenanceRainFitError::Model)?;
    if text(gradient, "name").as_deref() != Some("gbtree") {
        return Err(NativeMaintenanceRainFitError::Model);
    }
    let booster = field(gradient, "model").ok_or(NativeMaintenanceRainFitError::Model)?;
    let trees = array(booster, "trees")?;
    let tree_info = array(booster, "tree_info")?;
    if trees.len() != 160
        || tree_info.len() != 160
        || tree_info
            .iter()
            .any(|value| !matches!(value, JsonValue::Number(number) if *number == 0.0))
    {
        return Err(NativeMaintenanceRainFitError::Model);
    }
    let mut compact = Vec::with_capacity(160);
    // bind aligned noncategorical arrays for every native tree.
    for tree in trees {
        let split_types = array(tree, "split_type")?;
        let categories = array(tree, "categories")?;
        if !categories.is_empty()
            || split_types
                .iter()
                .any(|value| !matches!(value, JsonValue::Number(number) if *number == 0.0))
        {
            return Err(NativeMaintenanceRainFitError::Model);
        }
        let names = [
            "split_indices",
            "split_conditions",
            "left_children",
            "right_children",
            "default_left",
        ];
        let arrays = names
            .iter()
            .map(|name| {
                field(tree, name)
                    .cloned()
                    .ok_or(NativeMaintenanceRainFitError::Model)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let lengths = arrays
            .iter()
            .map(|value| match value {
                JsonValue::Array(values) => Some(values.len()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(NativeMaintenanceRainFitError::Model)?;
        if lengths
            .iter()
            .any(|length| *length == 0 || *length != lengths[0])
        {
            return Err(NativeMaintenanceRainFitError::Model);
        }
        compact.push(JsonValue::Array(arrays));
    }
    let parameters =
        field(learner, "learner_model_param").ok_or(NativeMaintenanceRainFitError::Model)?;
    let encoded_base =
        text(parameters, "base_score").ok_or(NativeMaintenanceRainFitError::Model)?;
    let parsed = serde_json::from_str::<Vec<f64>>(&encoded_base)
        .map_err(|_| NativeMaintenanceRainFitError::Model)?;
    if parsed.len() != 1 || !parsed[0].is_finite() {
        return Err(NativeMaintenanceRainFitError::Model);
    }
    Ok(object(vec![
        ("baseScore".to_owned(), number(parsed[0])),
        ("objective".to_owned(), string(expected)),
        ("trees".to_owned(), JsonValue::Array(compact)),
    ]))
}

/// Serialize one monthly fit state with the predecessor's exact public fields.
fn month_fit_json(fit: &MonthFit, month: &str) -> JsonValue {
    object(vec![
        (
            "calibration".to_owned(),
            fit.calibration
                .as_ref()
                .map(hurdle_calibration_json)
                .unwrap_or(JsonValue::Null),
        ),
        (
            "calibrationMaximumValidHourExclusive".to_owned(),
            number(fit.masks.calibration_stop as f64),
        ),
        (
            "calibrationStartHour".to_owned(),
            number(fit.masks.calibration_start as f64),
        ),
        (
            "decisionStartHour".to_owned(),
            number(fit.masks.decision_start as f64),
        ),
        (
            "decisionStopHourExclusive".to_owned(),
            number(fit.masks.decision_stop as f64),
        ),
        (
            "effectiveSupport".to_owned(),
            effective_support_json(fit.effective),
        ),
        ("legacyCalibrationDays".to_owned(), number(90.0)),
        ("maintenanceCalibrationDays".to_owned(), number(90.0)),
        ("month".to_owned(), string(month)),
        (
            "model".to_owned(),
            fit.model
                .as_ref()
                .map(model_state_json)
                .unwrap_or(JsonValue::Null),
        ),
        (
            "nestingSafetyFallback".to_owned(),
            fit.nesting_fallback
                .map(JsonValue::Boolean)
                .unwrap_or(JsonValue::Null),
        ),
        (
            "proposedRules".to_owned(),
            fit.proposed_rules
                .as_ref()
                .map(|rules| event_rules_json(rules))
                .unwrap_or(JsonValue::Null),
        ),
        ("reason".to_owned(), string(fit.reason)),
        (
            "support".to_owned(),
            object(vec![
                (
                    "calibration".to_owned(),
                    support_json(fit.calibration_support),
                ),
                ("training".to_owned(), support_json(fit.training_support)),
            ]),
        ),
        ("supported".to_owned(), JsonValue::Boolean(fit.supported)),
        (
            "trainingMaximumValidHourExclusive".to_owned(),
            number(fit.masks.fit_stop as f64),
        ),
        (
            "uniformRules".to_owned(),
            fit.uniform_rules
                .as_ref()
                .map(|rules| event_rules_json(rules))
                .unwrap_or(JsonValue::Null),
        ),
    ])
}

/// Serialize one four-head native model state.
fn model_state_json(model: &OrdinalModel) -> JsonValue {
    let wire = NativeWirePrimitives;
    let feature_names = wire
        .decode_json(RETAINED_RAIN_ARTIFACT)
        .and_then(|value| field(&value, "featureNames").cloned())
        .unwrap_or(JsonValue::Null);
    object(vec![
        ("featureNames".to_owned(), feature_names),
        (
            "heads".to_owned(),
            object(
                model
                    .heads
                    .iter()
                    .enumerate()
                    .map(|(index, head)| {
                        (
                            HEAD_NAMES[index].to_owned(),
                            object(vec![
                                (
                                    "modelFile".to_owned(),
                                    head.bytes
                                        .as_ref()
                                        .map(|_| string(MODEL_FILES[index]))
                                        .unwrap_or(JsonValue::Null),
                                ),
                                ("objective".to_owned(), string(head.objective)),
                                ("reason".to_owned(), string(head.reason)),
                                (
                                    "sha256".to_owned(),
                                    head.bytes
                                        .as_ref()
                                        .map(|bytes| string(&digest(bytes)))
                                        .unwrap_or(JsonValue::Null),
                                ),
                                ("support".to_owned(), head_support_json(head.support)),
                            ]),
                        )
                    })
                    .collect(),
            ),
        ),
        ("rounds".to_owned(), number(160.0)),
    ])
}

/// Serialize the complete hurdle calibration state.
fn hurdle_calibration_json(state: &HurdleCalibration) -> JsonValue {
    object(vec![
        (
            "calibrationRows".to_owned(),
            number(state.calibration_rows as f64),
        ),
        (
            "calibrationStopHourExclusive".to_owned(),
            number(state.calibration_stop as f64),
        ),
        (
            "calibrationSupport".to_owned(),
            object(vec![
                ("dates".to_owned(), number(state.dates as f64)),
                ("effectiveDates".to_owned(), number(state.effective.dates)),
                (
                    "effectiveWetDates".to_owned(),
                    number(state.effective.wet_dates),
                ),
                (
                    "observedWetDates".to_owned(),
                    number(state.observed_wet_dates as f64),
                ),
                (
                    "observedWetHours".to_owned(),
                    number(state.observed_wet_hours as f64),
                ),
                (
                    "observedWetRows".to_owned(),
                    number(state.observed_wet_rows as f64),
                ),
                ("rows".to_owned(), number(state.rows as f64)),
            ]),
        ),
        (
            "categories".to_owned(),
            object(
                state
                    .categories
                    .iter()
                    .enumerate()
                    .map(|(index, category)| {
                        ((index + 1).to_string(), category_calibration_json(category))
                    })
                    .collect(),
            ),
        ),
        ("contractVersion".to_owned(), string(HURDLE_VERSION)),
        (
            "globalCalibration".to_owned(),
            scalar_calibration_json(&state.global),
        ),
        ("productionEligible".to_owned(), JsonValue::Boolean(false)),
        ("rules".to_owned(), event_rules_json(&state.rules)),
        (
            "scaleBounds".to_owned(),
            JsonValue::Array(vec![number(0.1), number(3.0)]),
        ),
        (
            "uniformRules".to_owned(),
            event_rules_json(&state.uniform_rules),
        ),
        (
            "wetSelection".to_owned(),
            wet_selection_json(&state.wet_selection),
        ),
    ])
}

/// Serialize one category-specific amount calibration.
fn category_calibration_json(state: &CategoryCalibration) -> JsonValue {
    object(vec![
        (
            "calibration".to_owned(),
            state
                .calibration
                .as_ref()
                .map(scalar_calibration_json)
                .unwrap_or(JsonValue::Null),
        ),
        ("reason".to_owned(), string(state.reason)),
        ("scale".to_owned(), number(state.scale)),
        (
            "support".to_owned(),
            object(vec![
                (
                    "observedDryRows".to_owned(),
                    number(state.observed_dry_rows as f64),
                ),
                (
                    "observedWetDates".to_owned(),
                    number(state.observed_wet_dates as f64),
                ),
                (
                    "observedWetHours".to_owned(),
                    number(state.observed_wet_hours as f64),
                ),
                (
                    "observedWetRows".to_owned(),
                    number(state.observed_wet_rows as f64),
                ),
                (
                    "predictedDates".to_owned(),
                    number(state.predicted_dates as f64),
                ),
                (
                    "predictedHours".to_owned(),
                    number(state.predicted_hours as f64),
                ),
                ("rows".to_owned(), number(state.rows as f64)),
            ]),
        ),
        ("supported".to_owned(), JsonValue::Boolean(state.supported)),
    ])
}

/// Serialize one scalar calibration record.
fn scalar_calibration_json(state: &ScalarCalibration) -> JsonValue {
    object(vec![
        ("achievedMean".to_owned(), number(state.achieved_mean)),
        ("lowerMean".to_owned(), number(state.low_mean)),
        ("residual".to_owned(), number(state.residual)),
        ("scale".to_owned(), number(state.scale)),
        ("status".to_owned(), string(state.status)),
        ("targetMean".to_owned(), number(state.target_mean)),
        ("tolerance".to_owned(), number(state.tolerance)),
        ("upperMean".to_owned(), number(state.high_mean)),
    ])
}

/// Serialize one wet-rule search summary.
fn wet_selection_json(state: &WetSelection) -> JsonValue {
    object(vec![
        (
            "candidateScores".to_owned(),
            number(state.candidate_scores as f64),
        ),
        (
            "feasibleScores".to_owned(),
            number(state.feasible_scores as f64),
        ),
        (
            "oldNestedMetrics".to_owned(),
            event_metrics_json(state.old_nested_metrics),
        ),
        (
            "rawMetrics".to_owned(),
            event_metrics_json(state.raw_metrics),
        ),
        ("reason".to_owned(), string(state.reason)),
        (
            "selectedCutoff".to_owned(),
            nullable_number(state.selected_cutoff),
        ),
        (
            "selectedMetrics".to_owned(),
            event_metrics_json(state.selected_metrics),
        ),
        (
            "support".to_owned(),
            object(vec![
                (
                    "positiveDates".to_owned(),
                    number(state.positive_dates as f64),
                ),
                (
                    "positiveHours".to_owned(),
                    number(state.positive_hours as f64),
                ),
            ]),
        ),
    ])
}

/// Serialize all ordered event rules.
fn event_rules_json(rules: &[EventRule]) -> JsonValue {
    JsonValue::Array(rules.iter().map(event_rule_json).collect())
}

/// Serialize one calibrated or raw-fallback event rule.
fn event_rule_json(rule: &EventRule) -> JsonValue {
    object(vec![
        (
            "candidate".to_owned(),
            rule.candidate
                .map(event_metrics_json)
                .unwrap_or(JsonValue::Null),
        ),
        ("cutoff".to_owned(), nullable_number(rule.cutoff)),
        ("head".to_owned(), string(rule.head)),
        ("raw".to_owned(), event_metrics_json(rule.raw)),
        ("reason".to_owned(), string(rule.reason)),
        ("support".to_owned(), head_support_json(rule.support)),
        (
            "targetRecall".to_owned(),
            nullable_number(rule.target_recall),
        ),
        ("threshold".to_owned(), number(rule.threshold)),
    ])
}

/// Serialize one three-metric event record.
fn event_metrics_json(metrics: EventMetrics) -> JsonValue {
    object(vec![
        ("csi".to_owned(), number(metrics.csi)),
        ("far".to_owned(), number(metrics.far)),
        ("pod".to_owned(), number(metrics.pod)),
    ])
}

/// Serialize threshold support.
fn head_support_json(support: HeadSupport) -> JsonValue {
    object(vec![
        (
            "positiveDates".to_owned(),
            number(support.positive_dates as f64),
        ),
        (
            "positiveHours".to_owned(),
            number(support.positive_hours as f64),
        ),
        (
            "positiveRows".to_owned(),
            number(support.positive_rows as f64),
        ),
        ("rows".to_owned(), number(support.rows as f64)),
    ])
}

/// Serialize complete row/date/hour support.
fn support_json(support: Support) -> JsonValue {
    object(vec![
        ("dates".to_owned(), number(support.dates as f64)),
        ("hours".to_owned(), number(support.hours as f64)),
        ("rows".to_owned(), number(support.rows as f64)),
        ("wetDates".to_owned(), number(support.wet_dates as f64)),
        ("wetHours".to_owned(), number(support.wet_hours as f64)),
    ])
}

/// Serialize Kish-effective date support.
fn effective_support_json(support: EffectiveSupport) -> JsonValue {
    object(vec![
        ("effectiveDates".to_owned(), number(support.dates)),
        ("effectiveWetDates".to_owned(), number(support.wet_dates)),
    ])
}

type DMatrixHandle = *mut c_void;
type BoosterHandle = *mut c_void;
type XgbVersion = unsafe extern "C" fn(*mut c_int, *mut c_int, *mut c_int);
type XgbLastError = unsafe extern "C" fn() -> *const c_char;
type DMatrixCreateDense =
    unsafe extern "C" fn(*const c_char, *const c_char, *mut DMatrixHandle) -> c_int;
type DMatrixSetInfo = unsafe extern "C" fn(DMatrixHandle, *const c_char, *const c_char) -> c_int;
type DMatrixFree = unsafe extern "C" fn(DMatrixHandle) -> c_int;
type BoosterCreate = unsafe extern "C" fn(*const DMatrixHandle, u64, *mut BoosterHandle) -> c_int;
type BoosterSetFeatureInfo =
    unsafe extern "C" fn(BoosterHandle, *const c_char, *const *const c_char, u64) -> c_int;
type BoosterSetParam = unsafe extern "C" fn(BoosterHandle, *const c_char, *const c_char) -> c_int;
type BoosterUpdate = unsafe extern "C" fn(BoosterHandle, c_int, DMatrixHandle) -> c_int;
type BoosterReset = unsafe extern "C" fn(BoosterHandle) -> c_int;
type BoosterPredict = unsafe extern "C" fn(
    BoosterHandle,
    DMatrixHandle,
    *const c_char,
    *mut *const u64,
    *mut u64,
    *mut *const f32,
) -> c_int;
type BoosterSave =
    unsafe extern "C" fn(BoosterHandle, *const c_char, *mut u64, *mut *const c_char) -> c_int;
type BoosterFree = unsafe extern "C" fn(BoosterHandle) -> c_int;

struct XgBoost {
    booster_create: BoosterCreate,
    booster_free: BoosterFree,
    booster_predict: BoosterPredict,
    booster_reset: BoosterReset,
    booster_save: BoosterSave,
    booster_set_feature_info: BoosterSetFeatureInfo,
    booster_set_param: BoosterSetParam,
    booster_update: BoosterUpdate,
    dmatrix_create_dense: DMatrixCreateDense,
    dmatrix_free: DMatrixFree,
    dmatrix_set_info: DMatrixSetInfo,
    get_last_error: XgbLastError,
    library: *mut c_void,
}

struct XgDMatrix {
    free: DMatrixFree,
    handle: DMatrixHandle,
}

struct XgBooster {
    free: BoosterFree,
    handle: BoosterHandle,
}

impl Drop for XgDMatrix {
    /// Release one native matrix before its library owner is unloaded.
    fn drop(&mut self) {
        // the handle came from this exact pinned library.
        unsafe {
            (self.free)(self.handle);
        }
    }
}

impl Drop for XgBooster {
    /// Release one native booster before its library owner is unloaded.
    fn drop(&mut self) {
        // the handle came from this exact pinned library.
        unsafe {
            (self.free)(self.handle);
        }
    }
}

impl Drop for XgBoost {
    /// Unload the pinned library only after every model and matrix is gone.
    fn drop(&mut self) {
        // this pointer is the successful fixed-path dlopen result.
        unsafe {
            libc::dlclose(self.library);
        }
    }
}

impl XgBoost {
    /// Load and version-check the pinned existing x86_64 XGBoost library.
    fn load() -> Result<Self, NativeMaintenanceRainFitError> {
        Self::load_at(NATIVE_MAINTENANCE_RAIN_XGBOOST_PATH)
    }

    /// Load one exact library path for production or isolated parity tests.
    fn load_at(path: &str) -> Result<Self, NativeMaintenanceRainFitError> {
        #[cfg(not(target_arch = "x86_64"))]
        {
            return Err(NativeMaintenanceRainFitError::Runtime);
        }
        #[cfg(target_arch = "x86_64")]
        {
            let path = CString::new(path).map_err(|_| NativeMaintenanceRainFitError::Runtime)?;
            // bind only the fixed sandbox path and resolve every required stable C symbol.
            let library = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if library.is_null() {
                return Err(NativeMaintenanceRainFitError::Runtime);
            }
            macro_rules! symbol {
                ($name:literal, $kind:ty) => {{
                    let name = CString::new($name).expect("static symbol has no nul");
                    // the exact symbol types are copied from XGBoost's stable C API.
                    let pointer = unsafe { libc::dlsym(library, name.as_ptr()) };
                    if pointer.is_null() {
                        // release a partially admitted library before failing closed.
                        unsafe { libc::dlclose(library) };
                        return Err(NativeMaintenanceRainFitError::Runtime);
                    }
                    // dlsym function pointers have the declared stable ABI signature.
                    unsafe { std::mem::transmute::<*mut c_void, $kind>(pointer) }
                }};
            }
            let version: XgbVersion = symbol!("XGBoostVersion", XgbVersion);
            let mut major = 0;
            let mut minor = 0;
            let mut patch = 0;
            // version writes exactly three caller-owned integers.
            unsafe { version(&mut major, &mut minor, &mut patch) };
            if (major, minor, patch) != (3, 4, 1) {
                // prohibit a same-SONAME numerical runtime substitution.
                unsafe { libc::dlclose(library) };
                return Err(NativeMaintenanceRainFitError::Runtime);
            }
            Ok(Self {
                booster_create: symbol!("XGBoosterCreate", BoosterCreate),
                booster_free: symbol!("XGBoosterFree", BoosterFree),
                booster_predict: symbol!("XGBoosterPredictFromDMatrix", BoosterPredict),
                booster_reset: symbol!("XGBoosterReset", BoosterReset),
                booster_save: symbol!("XGBoosterSaveModelToBuffer", BoosterSave),
                booster_set_feature_info: symbol!(
                    "XGBoosterSetStrFeatureInfo",
                    BoosterSetFeatureInfo
                ),
                booster_set_param: symbol!("XGBoosterSetParam", BoosterSetParam),
                booster_update: symbol!("XGBoosterUpdateOneIter", BoosterUpdate),
                dmatrix_create_dense: symbol!("XGDMatrixCreateFromDense", DMatrixCreateDense),
                dmatrix_free: symbol!("XGDMatrixFree", DMatrixFree),
                dmatrix_set_info: symbol!("XGDMatrixSetInfoFromInterface", DMatrixSetInfo),
                get_last_error: symbol!("XGBGetLastError", XgbLastError),
                library,
            })
        }
    }

    /// Train one exact source objective through the stable C API.
    fn train(
        &self,
        features: &[f32],
        rows: usize,
        labels: &[f64],
        weights: &[f64],
        objective: &str,
    ) -> Result<XgBooster, NativeMaintenanceRainFitError> {
        if rows == 0
            || features.len() != rows * FEATURE_COUNT
            || labels.len() != rows
            || weights.len() != rows
        {
            return Err(NativeMaintenanceRainFitError::Numeric);
        }
        let matrix = self.matrix(features, rows)?;
        self.set_matrix_info(matrix.handle, "label", labels, "<f8")?;
        self.set_matrix_info(matrix.handle, "weight", weights, "<f8")?;
        let handles = [matrix.handle];
        let mut handle = ptr::null_mut();
        // create a booster with the training matrix in its source cache.
        self.call(unsafe { (self.booster_create)(handles.as_ptr(), 1, &mut handle) })?;
        let booster = XgBooster {
            free: self.booster_free,
            handle,
        };
        self.set_feature_names(booster.handle)?;
        for (key, value) in [
            ("tree_method", "hist"),
            ("device", "cpu"),
            ("max_depth", "3"),
            ("eta", "0.04"),
            ("min_child_weight", "25"),
            ("lambda", "20"),
            ("subsample", "1"),
            ("colsample_bytree", "1"),
            ("seed", "20260913"),
            ("nthread", "1"),
            ("objective", objective),
            ("validate_parameters", "True"),
        ] {
            self.set_param(booster.handle, key, value)?;
        }
        // preserve the source's exact 160 boosting rounds.
        for iteration in 0..160 {
            self.call(unsafe { (self.booster_update)(booster.handle, iteration, matrix.handle) })?;
        }
        self.call(unsafe { (self.booster_reset)(booster.handle) })?;
        Ok(booster)
    }

    /// Create one C-contiguous f32 dense matrix with NaN missing values.
    fn matrix(
        &self,
        features: &[f32],
        rows: usize,
    ) -> Result<XgDMatrix, NativeMaintenanceRainFitError> {
        let interface = dense_interface(features.as_ptr() as usize, &[rows, FEATURE_COUNT], "<f4")?;
        let config = CString::new("{\"missing\":NaN,\"nthread\":1}")
            .map_err(|_| NativeMaintenanceRainFitError::Runtime)?;
        let mut handle = ptr::null_mut();
        self.call(unsafe {
            (self.dmatrix_create_dense)(interface.as_ptr(), config.as_ptr(), &mut handle)
        })?;
        Ok(XgDMatrix {
            free: self.dmatrix_free,
            handle,
        })
    }

    /// Set one dense f64 matrix metadata vector.
    fn set_matrix_info(
        &self,
        matrix: DMatrixHandle,
        field_name: &str,
        values: &[f64],
        typestr: &str,
    ) -> Result<(), NativeMaintenanceRainFitError> {
        let name = CString::new(field_name).map_err(|_| NativeMaintenanceRainFitError::Runtime)?;
        let interface = dense_interface(values.as_ptr() as usize, &[values.len()], typestr)?;
        self.call(unsafe { (self.dmatrix_set_info)(matrix, name.as_ptr(), interface.as_ptr()) })
    }

    /// Set the reviewed feature-name order on one booster.
    fn set_feature_names(
        &self,
        booster: BoosterHandle,
    ) -> Result<(), NativeMaintenanceRainFitError> {
        let wire = NativeWirePrimitives;
        let retained = wire
            .decode_json(RETAINED_RAIN_ARTIFACT)
            .ok_or(NativeMaintenanceRainFitError::Codec)?;
        let JsonValue::Array(names) =
            field(&retained, "featureNames").ok_or(NativeMaintenanceRainFitError::Contract)?
        else {
            return Err(NativeMaintenanceRainFitError::Contract);
        };
        let strings = names
            .iter()
            .map(|value| match value {
                JsonValue::String(value) => String::from_utf16(value).ok(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(NativeMaintenanceRainFitError::Contract)?;
        let names = strings
            .iter()
            .map(|value| CString::new(value.as_str()).ok())
            .collect::<Option<Vec<_>>>()
            .ok_or(NativeMaintenanceRainFitError::Contract)?;
        let pointers = names.iter().map(|value| value.as_ptr()).collect::<Vec<_>>();
        let field = CString::new("feature_name").expect("static feature field has no nul");
        self.call(unsafe {
            (self.booster_set_feature_info)(
                booster,
                field.as_ptr(),
                pointers.as_ptr(),
                pointers.len() as u64,
            )
        })
    }

    /// Set one scalar booster parameter.
    fn set_param(
        &self,
        booster: BoosterHandle,
        key: &str,
        value: &str,
    ) -> Result<(), NativeMaintenanceRainFitError> {
        let key = CString::new(key).map_err(|_| NativeMaintenanceRainFitError::Runtime)?;
        let value = CString::new(value).map_err(|_| NativeMaintenanceRainFitError::Runtime)?;
        self.call(unsafe { (self.booster_set_param)(booster, key.as_ptr(), value.as_ptr()) })
    }

    /// Map one C API status into the closed model failure surface.
    fn call(&self, status: c_int) -> Result<(), NativeMaintenanceRainFitError> {
        if status == 0 {
            return Ok(());
        }
        // force the library to materialize its thread-local diagnostic before closing it.
        unsafe {
            let pointer = (self.get_last_error)();
            if !pointer.is_null() {
                let _ = CStr::from_ptr(pointer).to_bytes();
            }
        }
        Err(NativeMaintenanceRainFitError::Model)
    }
}

impl XgBooster {
    /// Predict one dense matrix with the source's default value prediction config.
    fn predict(
        &self,
        xgboost: &XgBoost,
        features: &[f32],
        rows: usize,
    ) -> Result<Vec<f32>, NativeMaintenanceRainFitError> {
        let matrix = xgboost.matrix(features, rows)?;
        let config = CString::new(
            "{\"type\":0,\"training\":false,\"iteration_begin\":0,\"iteration_end\":0,\"strict_shape\":false}",
        )
        .map_err(|_| NativeMaintenanceRainFitError::Runtime)?;
        let mut shape = ptr::null();
        let mut dimensions = 0_u64;
        let mut output = ptr::null();
        xgboost.call(unsafe {
            (xgboost.booster_predict)(
                self.handle,
                matrix.handle,
                config.as_ptr(),
                &mut shape,
                &mut dimensions,
                &mut output,
            )
        })?;
        if dimensions != 1 || shape.is_null() || output.is_null() {
            return Err(NativeMaintenanceRainFitError::Model);
        }
        // the prediction buffer remains owned by the booster until the next API call.
        let length = unsafe { *shape } as usize;
        if length != rows {
            return Err(NativeMaintenanceRainFitError::Model);
        }
        Ok(unsafe { slice::from_raw_parts(output, length) }.to_vec())
    }

    /// Save one exact JSON model buffer including feature names.
    fn save_json(&self, xgboost: &XgBoost) -> Result<Vec<u8>, NativeMaintenanceRainFitError> {
        let config = CString::new("{\"format\":\"json\"}")
            .map_err(|_| NativeMaintenanceRainFitError::Runtime)?;
        let mut length = 0_u64;
        let mut output = ptr::null();
        xgboost.call(unsafe {
            (xgboost.booster_save)(self.handle, config.as_ptr(), &mut length, &mut output)
        })?;
        if output.is_null() || length == 0 {
            return Err(NativeMaintenanceRainFitError::Model);
        }
        Ok(unsafe { slice::from_raw_parts(output.cast::<u8>(), length as usize) }.to_vec())
    }
}

/// Build one NumPy-compatible array interface for a caller-owned buffer.
fn dense_interface(
    pointer: usize,
    shape: &[usize],
    typestr: &str,
) -> Result<CString, NativeMaintenanceRainFitError> {
    let shape = shape
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    CString::new(format!(
        "{{\"data\":[{pointer},false],\"shape\":[{shape}],\"strides\":null,\"typestr\":\"{typestr}\",\"version\":3}}"
    ))
    .map_err(|_| NativeMaintenanceRainFitError::Runtime)
}

/// Select one flattened f32 feature matrix in source row order.
fn select_features(rows: &[RainRow], indices: &[usize]) -> Vec<f32> {
    let mut values = Vec::with_capacity(indices.len() * FEATURE_COUNT);
    // retain every explicit NaN and finite float32 cell.
    for index in indices {
        values.extend_from_slice(&rows[*index].features);
    }
    values
}

/// Select one numerical row member in source order.
fn select_numbers(
    rows: &[RainRow],
    indices: &[usize],
    select: impl Fn(&RainRow) -> f64,
) -> Vec<f64> {
    indices.iter().map(|index| select(&rows[*index])).collect()
}

/// Select source valid hours in row order.
fn select_hours(rows: &[RainRow], indices: &[usize]) -> Vec<i64> {
    indices.iter().map(|index| rows[*index].hour).collect()
}

/// Select the exact decision-month evaluation population.
fn evaluation_indices(rows: &[RainRow], masks: MonthMasks) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let decision = row.initialized + 8;
            (decision >= masks.decision_start && decision < masks.decision_stop).then_some(index)
        })
        .collect()
}

/// Resolve one UTC hour to its UTC year-month label.
fn month_from_hour(hour: i64) -> Result<String, NativeMaintenanceRainFitError> {
    let timestamp = Timestamp::from_millisecond(hour * 3_600_000)
        .map_err(|_| NativeMaintenanceRainFitError::Clock)?;
    let datetime = TimeZone::UTC.to_datetime(timestamp);
    Ok(format!("{:04}-{:02}", datetime.year(), datetime.month()))
}

/// Resolve one UTC hour to its Los Angeles calendar month.
fn local_month(hour: i64) -> Result<u8, NativeMaintenanceRainFitError> {
    let timestamp = Timestamp::from_millisecond(hour * 3_600_000)
        .map_err(|_| NativeMaintenanceRainFitError::Clock)?;
    let timezone = LOS_ANGELES
        .get_or_init(|| load_named_timezone("America/Los_Angeles"))
        .as_ref()
        .ok_or(NativeMaintenanceRainFitError::Clock)?;
    Ok(timezone.to_datetime(timestamp).month() as u8)
}

/// Resolve one UTC hour to its Los Angeles ISO calendar date.
fn local_date(hour: i64) -> Result<String, NativeMaintenanceRainFitError> {
    let timestamp = Timestamp::from_millisecond(hour * 3_600_000)
        .map_err(|_| NativeMaintenanceRainFitError::Clock)?;
    let timezone = LOS_ANGELES
        .get_or_init(|| load_named_timezone("America/Los_Angeles"))
        .as_ref()
        .ok_or(NativeMaintenanceRainFitError::Clock)?;
    Ok(timezone.to_datetime(timestamp).date().to_string())
}

/// Parse one exact canonical millisecond UTC instant.
fn canonical_milliseconds(
    value: &str,
    wire: &NativeWirePrimitives,
) -> Result<i64, NativeMaintenanceRainFitError> {
    let milliseconds = wire
        .parse_explicit_utc_milliseconds(value)
        .ok_or(NativeMaintenanceRainFitError::Clock)?;
    if wire.format_utc_milliseconds(milliseconds).as_deref() != Some(value) {
        return Err(NativeMaintenanceRainFitError::Clock);
    }
    Ok(milliseconds)
}

/// Parse the source receipt clock through the same canonical instant owner.
fn canonical_hour_or_millisecond(
    value: &str,
    wire: &NativeWirePrimitives,
) -> Result<i64, NativeMaintenanceRainFitError> {
    canonical_milliseconds(value, wire)
}

/// Borrow one required array field.
fn array<'a>(
    value: &'a JsonValue,
    name: &str,
) -> Result<&'a Vec<JsonValue>, NativeMaintenanceRainFitError> {
    match field(value, name) {
        Some(JsonValue::Array(values)) => Ok(values),
        _ => Err(NativeMaintenanceRainFitError::Input),
    }
}

/// Decode one exact finite number.
fn finite_number(value: &JsonValue, name: &str) -> Result<f64, NativeMaintenanceRainFitError> {
    match field(value, name) {
        Some(JsonValue::Number(number)) if number.is_finite() => Ok(*number),
        _ => Err(NativeMaintenanceRainFitError::Numeric),
    }
}

/// Decode one exact finite nonnegative number.
fn finite_nonnegative(value: &JsonValue, name: &str) -> Result<f64, NativeMaintenanceRainFitError> {
    let number = finite_number(value, name)?;
    if number < 0.0 {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    Ok(number)
}

/// Decode one nullable finite nonnegative number.
fn nullable_nonnegative(
    value: &JsonValue,
    name: &str,
) -> Result<Option<f64>, NativeMaintenanceRainFitError> {
    match field(value, name) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::Number(number)) if number.is_finite() && *number >= 0.0 => {
            Ok(Some(*number))
        }
        _ => Err(NativeMaintenanceRainFitError::Numeric),
    }
}

/// Decode one fixed-width finite-or-null f32 vector.
fn numbers_or_nulls(
    value: &JsonValue,
    name: &str,
    length: usize,
) -> Result<Vec<f32>, NativeMaintenanceRainFitError> {
    let values = array(value, name)?;
    if values.len() != length {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    values
        .iter()
        .map(|value| match value {
            JsonValue::Null => Ok(f32::NAN),
            JsonValue::Number(number) if number.is_finite() => Ok(*number as f32),
            _ => Err(NativeMaintenanceRainFitError::Numeric),
        })
        .collect()
}

/// Decode one exact nonnegative safe integer.
fn exact_u64(value: &JsonValue, name: &str) -> Result<u64, NativeMaintenanceRainFitError> {
    let number = finite_number(value, name)?;
    if number < 0.0 || number.fract() != 0.0 || number > 9_007_199_254_740_991.0 {
        return Err(NativeMaintenanceRainFitError::Numeric);
    }
    Ok(number as u64)
}

/// Decode one exact platform-sized integer.
fn exact_usize(value: &JsonValue, name: &str) -> Result<usize, NativeMaintenanceRainFitError> {
    usize::try_from(exact_u64(value, name)?).map_err(|_| NativeMaintenanceRainFitError::Numeric)
}

/// Decode one required boolean.
fn boolean(value: &JsonValue, name: &str) -> Result<bool, NativeMaintenanceRainFitError> {
    match field(value, name) {
        Some(JsonValue::Boolean(value)) => Ok(*value),
        _ => Err(NativeMaintenanceRainFitError::Input),
    }
}

/// Decode one integral number array.
fn number_array(value: &JsonValue, name: &str) -> Result<Vec<u64>, NativeMaintenanceRainFitError> {
    array(value, name)?
        .iter()
        .map(|value| match value {
            JsonValue::Number(number)
                if number.is_finite() && *number >= 0.0 && number.fract() == 0.0 =>
            {
                Ok(*number as u64)
            }
            _ => Err(NativeMaintenanceRainFitError::Numeric),
        })
        .collect()
}

/// Hash one recursively sorted LF-terminated JSON value.
fn hash_json(value: &JsonValue) -> Result<String, NativeMaintenanceRainFitError> {
    let wire = NativeWirePrimitives;
    let bytes = wire
        .encode_sorted_json(value)
        .ok_or(NativeMaintenanceRainFitError::Codec)?;
    Ok(digest(&bytes))
}

/// Encode Python's sorted compact JSON shape while preserving f64 number typing.
fn encode_python_sorted_json(value: &JsonValue) -> Result<Vec<u8>, NativeMaintenanceRainFitError> {
    let value = python_json_value(value, &mut Vec::new())?;
    let mut bytes = Vec::new();
    write_python_json(&value, &mut bytes)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Write sorted JSON with Python's shortest finite float spelling.
fn write_python_json(
    value: &serde_json::Value,
    output: &mut Vec<u8>,
) -> Result<(), NativeMaintenanceRainFitError> {
    match value {
        serde_json::Value::Null => output.extend_from_slice(b"null"),
        serde_json::Value::Bool(true) => output.extend_from_slice(b"true"),
        serde_json::Value::Bool(false) => output.extend_from_slice(b"false"),
        serde_json::Value::Number(value) => {
            output.extend_from_slice(python_json_number(value)?.as_bytes());
        }
        serde_json::Value::String(value) => output.extend_from_slice(
            serde_json::to_string(value)
                .map_err(|_| NativeMaintenanceRainFitError::Codec)?
                .as_bytes(),
        ),
        serde_json::Value::Array(values) => {
            output.push(b'[');
            // preserve source array order.
            for (index, value) in values.iter().enumerate() {
                // separate compact values.
                if index > 0 {
                    output.push(b',');
                }
                write_python_json(value, output)?;
            }
            output.push(b']');
        }
        serde_json::Value::Object(fields) => {
            output.push(b'{');
            // preserve the already sorted unique key order.
            for (index, (key, value)) in fields.iter().enumerate() {
                // separate compact fields.
                if index > 0 {
                    output.push(b',');
                }
                output.extend_from_slice(
                    serde_json::to_string(key)
                        .map_err(|_| NativeMaintenanceRainFitError::Codec)?
                        .as_bytes(),
                );
                output.push(b':');
                write_python_json(value, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

/// Render one admitted number with Python's exponent sign and width.
fn python_json_number(value: &serde_json::Number) -> Result<String, NativeMaintenanceRainFitError> {
    // retain explicitly typed source integers.
    if let Some(value) = value.as_i64() {
        return Ok(value.to_string());
    }
    let value = value
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or(NativeMaintenanceRainFitError::Numeric)?;
    let source = serde_json::to_string(&value).map_err(|_| NativeMaintenanceRainFitError::Codec)?;
    let Some((coefficient, exponent)) = source.split_once(['e', 'E']) else {
        return Ok(if source.contains('.') {
            source
        } else {
            format!("{source}.0")
        });
    };
    let exponent = exponent
        .parse::<i32>()
        .map_err(|_| NativeMaintenanceRainFitError::Codec)?;
    // retain Python's explicit exponent sign.
    Ok(format!(
        "{coefficient}e{}{absolute:02}",
        if exponent >= 0 { '+' } else { '-' },
        absolute = exponent.unsigned_abs()
    ))
}

/// Convert one contract value into a recursively sorted serde JSON value.
fn python_json_value(
    value: &JsonValue,
    path: &mut Vec<PythonJsonPath>,
) -> Result<serde_json::Value, NativeMaintenanceRainFitError> {
    Ok(match value {
        JsonValue::Boolean(value) => serde_json::Value::Bool(*value),
        JsonValue::Number(value) => {
            if python_json_integer(path, *value) {
                serde_json::Value::Number(serde_json::Number::from(*value as i64))
            } else {
                serde_json::Value::Number(
                    serde_json::Number::from_f64(*value)
                        .ok_or(NativeMaintenanceRainFitError::Codec)?,
                )
            }
        }
        JsonValue::String(value) => serde_json::Value::String(
            String::from_utf16(value).map_err(|_| NativeMaintenanceRainFitError::Codec)?,
        ),
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Array(values) => {
            let mut output = Vec::with_capacity(values.len());
            // preserve array position for closed tree-column typing.
            for (index, value) in values.iter().enumerate() {
                path.push(PythonJsonPath::Index(index));
                output.push(python_json_value(value, path)?);
                path.pop();
            }
            serde_json::Value::Array(output)
        }
        JsonValue::Object(fields) => {
            let mut sorted = BTreeMap::new();
            // reject duplicate or invalid UTF-16 keys before sorted serialization.
            for (name, value) in fields {
                let name =
                    String::from_utf16(name).map_err(|_| NativeMaintenanceRainFitError::Codec)?;
                path.push(PythonJsonPath::Key(name.clone()));
                let value = python_json_value(value, path)?;
                path.pop();
                if sorted.insert(name, value).is_some() {
                    return Err(NativeMaintenanceRainFitError::Codec);
                }
            }
            serde_json::Value::Object(sorted.into_iter().collect())
        }
    })
}

#[derive(Clone)]
enum PythonJsonPath {
    Index(usize),
    Key(String),
}

/// Retain the source's closed integer fields and compact tree index columns.
fn python_json_integer(path: &[PythonJsonPath], value: f64) -> bool {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < i64::MIN as f64
        || value > i64::MAX as f64
    {
        return false;
    }
    if value == 0.0
        && path
            .iter()
            .any(|part| matches!(part, PythonJsonPath::Key(key) if key == "ordinalArtifact"))
    {
        return true;
    }
    // retain integer values emitted by the JavaScript gate owner.
    if path
        .iter()
        .any(|part| matches!(part, PythonJsonPath::Key(key) if key == "gridReports"))
        && path
            .iter()
            .any(|part| matches!(part, PythonJsonPath::Key(key) if key == "gateReport"))
    {
        return true;
    }
    let key = path.iter().rev().find_map(|part| match part {
        PythonJsonPath::Key(key) => Some(key.as_str()),
        PythonJsonPath::Index(_) => None,
    });
    if let Some(key) = key {
        if matches!(key, "effectiveDates" | "effectiveWetDates") {
            return false;
        }
        if key == "features"
            && path
                .iter()
                .any(|part| matches!(part, PythonJsonPath::Key(key) if key == "parityRows"))
        {
            return false;
        }
        if matches!(
            key,
            "calibrationDays"
                | "embargoDays"
                | "features"
                | "legacyCalibrationDays"
                | "maintenanceCalibrationDays"
                | "minimumCalibrationDates"
                | "nthread"
                | "rounds"
                | "sourceFitterLegacyCalibrationDays"
                | "candidateScores"
                | "feasibleScores"
                | "hours"
                | "rows"
                | "dates"
                | "uniqueHours"
                | "wetDates"
                | "heavyHours"
        ) || key.ends_with("Count")
            || key.ends_with("Rows")
            || key.ends_with("Hours")
            || key.ends_with("Dates")
            || key.ends_with("Hour")
            || key.ends_with("HourExclusive")
        {
            return true;
        }
    }
    let Some(position) = path
        .iter()
        .rposition(|part| matches!(part, PythonJsonPath::Key(key) if key == "trees"))
    else {
        return false;
    };
    let [
        PythonJsonPath::Index(_tree),
        PythonJsonPath::Index(column),
        PythonJsonPath::Index(_cell),
    ] = path.get(position + 1..).unwrap_or_default()
    else {
        return false;
    };
    matches!(column, 0 | 2 | 3 | 4)
}

/// Hash exact bytes with lowercase SHA-256.
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Replace one existing object field without changing the field set.
fn set_field(
    value: &mut JsonValue,
    name: &str,
    replacement: JsonValue,
) -> Result<(), NativeMaintenanceRainFitError> {
    let JsonValue::Object(fields) = value else {
        return Err(NativeMaintenanceRainFitError::Input);
    };
    let Some((_, value)) = fields
        .iter_mut()
        .find(|(key, _)| key.iter().copied().eq(name.encode_utf16()))
    else {
        return Err(NativeMaintenanceRainFitError::Input);
    };
    *value = replacement;
    Ok(())
}

/// Build one native probability object.
fn probability_object(values: [f64; 3]) -> JsonValue {
    object(vec![
        ("atLeast0_1".to_owned(), number(values[0])),
        ("atLeast1_0".to_owned(), number(values[1])),
        ("atLeast2_5".to_owned(), number(values[2])),
    ])
}

/// Build one UTF-16-keyed JSON object.
fn object(fields: Vec<(String, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one UTF-16 JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one finite JSON number.
fn number(value: f64) -> JsonValue {
    JsonValue::Number(if value == 0.0 { 0.0 } else { value })
}

/// Build one nullable JSON number.
fn nullable_number(value: Option<f64>) -> JsonValue {
    value.map(number).unwrap_or(JsonValue::Null)
}

#[cfg(test)]
#[path = "native_maintenance_rain_fit_test.rs"]
mod tests;
