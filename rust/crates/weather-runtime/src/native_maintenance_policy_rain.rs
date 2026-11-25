//! Exact rain maintenance promotion, regression, and development policy evaluation.

use std::collections::{BTreeSet, HashMap, HashSet};

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_policy_comparison::{
    NativeMaintenancePairedComparison, NativeMaintenancePolicyComparator,
    NativeMaintenancePolicyRow, create_native_maintenance_policy_row,
    decode_native_maintenance_policy_row, native_maintenance_paired_comparison,
    native_maintenance_policy_population, native_maintenance_population_report, number, object,
    string,
};
use crate::native_maintenance_policy_temperature_wind::{
    Gate, GateState, NativeMaintenancePolicyKind, base_evidence_gates, boolean_gate, epoch_gate,
    finalize, number_text, pending_gate, support_boolean,
};
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceCaptureFamily, NativePredecessorModelCalendar,
    NativeWirePrimitives,
};

const POLICY_VERSION: &str = "rain-maintenance-policy/v2";
const DEVELOPMENT_POPULATION_VERSION: &str = "rain-maintenance-development-population/v3";
const HISTORICAL_ROW_COUNT: usize = 32_896;
const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const HOUR_MILLISECONDS: i64 = 3_600_000;
const RAIN_THRESHOLDS: [f64; 3] = [0.1, 1.0, 2.5];
const ACCUMULATION_HOURS: [usize; 3] = [6, 12, 23];
const RAIN_SEASONS: [(&str, &str); 4] = [
    ("DJF", "winter"),
    ("MAM", "spring"),
    ("JJA", "summer"),
    ("SON", "autumn"),
];
const RAIN_LEADS: [(&str, u16, u16); 3] = [("1-6", 1, 6), ("7-12", 7, 12), ("13-23", 13, 23)];
const PROBABILITY_KEYS: [&str; 3] = ["atLeast0_1", "atLeast1_0", "atLeast2_5"];
const STABLE_GATE_IDS: [&str; 49] = [
    "maeImprovesFivePercent",
    "beatsVolumeScale",
    "beatsPersistence",
    "rmseNoWorse",
    "wetIntensityNoWorse",
    "heavyIntensityBounded",
    "annualVolumeBalanced",
    "overallSupport",
    "allSeasonsPresent",
    "allLeadBandsPresent",
    "rawHeavyAmountsUnchanged",
    "rawWetCallsPreserved",
    "event0.1Safety",
    "event1.0Safety",
    "event2.5Safety",
    "seasonDJFSupport",
    "seasonDJFVolume",
    "seasonDJFMae",
    "seasonDJFWetHeavy",
    "seasonDJFDetection",
    "seasonMAMSupport",
    "seasonMAMVolume",
    "seasonMAMMae",
    "seasonMAMWetHeavy",
    "seasonMAMDetection",
    "seasonJJASupport",
    "seasonJJAVolume",
    "seasonJJAMae",
    "seasonJJAWetHeavy",
    "seasonJJADetection",
    "seasonSONSupport",
    "seasonSONVolume",
    "seasonSONMae",
    "seasonSONWetHeavy",
    "seasonSONDetection",
    "lead1-6Mae",
    "lead1-6Support",
    "lead7-12Mae",
    "lead7-12Support",
    "lead13-23Mae",
    "lead13-23Support",
    "accumulation6Mae",
    "accumulation12Mae",
    "accumulation23Mae",
    "beatsSameWindowVolumeScale",
    "beatsRecentVolumeScale",
    "beatsUnchangedOrdinal",
    "seasonalBalanceImproves",
    "heavySkillRetained",
];

/// One exact source probability head set.
#[derive(Clone, Copy, Debug)]
struct RainProbability {
    at_least_0_1: f64,
    at_least_1_0: f64,
    at_least_2_5: f64,
}

impl RainProbability {
    /// Select the source probability member for one fixed occurrence threshold.
    fn at(self, threshold: f64) -> f64 {
        if threshold == 0.1 {
            self.at_least_0_1
        } else if threshold == 1.0 {
            self.at_least_1_0
        } else {
            self.at_least_2_5
        }
    }
}

/// One physically admitted rain row plus its shared paired-comparison projection.
#[derive(Clone, Debug)]
struct RainRow {
    base: NativeMaintenancePolicyRow,
    candidate_probability: RainProbability,
    incumbent_probability: RainProbability,
    native_source_probability: RainProbability,
    persistence_prediction: f64,
    recent_volume_scale_prediction: f64,
    run_key: String,
    same_window_volume_scale_prediction: f64,
    unchanged_ordinal_prediction: f64,
    volume_scale_prediction: f64,
}

/// Scalar date-balanced rain diagnostics used by the stable gates.
#[derive(Clone, Copy, Debug)]
struct RainMetrics {
    candidate_mae: f64,
    candidate_rmse: f64,
    heavy_candidate_mae: Option<f64>,
    heavy_raw_mae: Option<f64>,
    raw_mae: f64,
    raw_rmse: f64,
    volume_ratio: Option<f64>,
    wet_candidate_mae: Option<f64>,
    wet_raw_mae: Option<f64>,
}

/// Deterministic occurrence diagnostics for one amount threshold.
#[derive(Clone, Copy, Debug)]
struct EventMetrics {
    csi: Option<f64>,
    far: Option<f64>,
    pod: Option<f64>,
}

/// Distinct-date and event support counts.
#[derive(Clone, Copy, Debug)]
struct RainSupport {
    dates: usize,
    heavy_hours: usize,
    wet_dates: usize,
}

/// Complete-window accumulation error diagnostics.
#[derive(Clone, Copy, Debug)]
struct RainAccumulation {
    candidate_mae: f64,
    raw_mae: f64,
    windows: usize,
}

/// Construct one hash-bound rain row while retaining source-owned family and pair fields.
pub fn create_native_rain_maintenance_policy_row(
    input: &JsonValue,
    calendar: &NativePredecessorModelCalendar,
) -> Option<NativeMaintenancePolicyRow> {
    let JsonValue::Object(fields) = input else {
        return None;
    };
    // only the rain constructor may supply these two fixed row members
    if field(input, "family").is_some() || field(input, "pairKey").is_some() {
        return None;
    }
    let mut fields = fields.clone();
    fields.push(("family".encode_utf16().collect(), string("rain")));
    fields.push(("pairKey".encode_utf16().collect(), JsonValue::Null));
    let row = create_native_maintenance_policy_row(&JsonValue::Object(fields), calendar)?;
    decode_rain_row(&row.value, calendar).map(|rain| rain.base)
}

/// Dispatch one rain promotion or regression policy through the shared mode contract.
pub fn evaluate_native_rain_maintenance_policy(
    kind: NativeMaintenancePolicyKind,
    rows: &[JsonValue],
    epoch: Option<&JsonValue>,
) -> Option<JsonValue> {
    match kind {
        NativeMaintenancePolicyKind::Promotion if epoch.is_none() => {
            evaluate_native_rain_maintenance_promotion(rows)
        }
        NativeMaintenancePolicyKind::Regression => {
            evaluate_native_rain_maintenance_regression(rows, epoch?)
        }
        NativeMaintenancePolicyKind::Promotion => None,
    }
}

/// Evaluate the complete rain promotion contract including the 49 stable gates.
pub fn evaluate_native_rain_maintenance_promotion(rows: &[JsonValue]) -> Option<JsonValue> {
    let rows = decode_rain_population(rows, Some(HISTORICAL_ROW_COUNT))?;
    let base = base_refs(&rows);
    let mut gates = base_evidence_gates(&base);
    let population = native_maintenance_population_report(&base)?;
    gates.push(skill_gate(
        "incumbent_skill",
        population.incumbent.as_ref(),
    )?);
    gates.extend(rain_stable_gates(&rows)?);
    gates.push(difference_gate(
        "actual_best_match_overall",
        population.actual_best_match_all_rows.as_ref(),
        0.0,
    ));
    // preserve every season and lead Best Match nonharm receipt in source order
    for (id, selected) in rain_best_match_slices(&rows) {
        let selected_base = base_refs(&selected);
        let comparison = comparison_or_null(
            &selected_base,
            NativeMaintenancePolicyComparator::ActualBestMatch,
        )?;
        // unsupported slices remain pending instead of passing null metrics
        let gate = if let Some(comparison) = comparison {
            boolean_gate(
                &format!("{id}:actual_best_match_nonharm"),
                comparison.candidate_mae <= comparison.comparator_mae * 1.05,
                JsonValue::Number(comparison.candidate_mae - comparison.comparator_mae),
                "candidate mae <= actual Best Match mae * 1.05",
            )
        } else {
            pending_gate(
                &format!("{id}:actual_best_match_nonharm"),
                "candidate mae <= actual Best Match mae * 1.05",
            )
        };
        gates.push(gate);
    }
    gates.push(rain_brier_gate(&rows));
    gates.push(rain_nesting_gate(&rows));
    gates.push(rain_coherence_gate(&rows));
    Some(finalize(
        POLICY_VERSION,
        NativeMaintenancePolicyKind::Promotion,
        population,
        gates,
    ))
}

/// Evaluate the one authorized rain regression member and all stable gates.
pub fn evaluate_native_rain_maintenance_regression(
    rows: &[JsonValue],
    epoch: &JsonValue,
) -> Option<JsonValue> {
    let rows = decode_rain_population(rows, Some(HISTORICAL_ROW_COUNT))?;
    let base = base_refs(&rows);
    let mut gates = base_evidence_gates(&base);
    gates.push(epoch_gate(epoch)?);
    let epoch_start = text(epoch, "epochStartInclusive")?;
    let epoch_end = text(epoch, "epochEndExclusive")?;
    let authorized = epoch_start.get(..10) == Some("2026-10-31")
        && epoch_end.get(..10) == Some("2027-09-30")
        && rows
            .iter()
            .all(|row| ("2026-10-31"..="2027-09-29").contains(&row.base.local_date.as_str()));
    gates.push(boolean_gate(
        "authorized_epoch",
        authorized,
        string(&format!("{epoch_start}/{epoch_end}")),
        "exact authorized 2026-10-31..2027-09-29 member",
    ));
    gates.extend(rain_stable_gates(&rows)?);
    let population = native_maintenance_population_report(&base)?;
    gates.push(regression_comparison_gate(
        "active_native_source_harm",
        population.native_source.as_ref(),
    )?);
    gates.push(regression_comparison_gate(
        "active_actual_best_match_harm",
        population.actual_best_match_all_rows.as_ref(),
    )?);
    Some(finalize(
        POLICY_VERSION,
        NativeMaintenancePolicyKind::Regression,
        population,
        gates,
    ))
}

/// Evaluate the frozen 32,896-row historical screen without production authority.
pub fn evaluate_native_rain_maintenance_development(rows: &[JsonValue]) -> Option<JsonValue> {
    let rows = decode_rain_population(rows, Some(HISTORICAL_ROW_COUNT))?;
    // forbid prospective receipts from being relabelled as development evidence
    if !rows
        .iter()
        .all(|row| row.base.evidence_class == "historical_development")
    {
        return None;
    }
    development_report(&rows, None)
}

/// Evaluate one complete future-only annual source population without action authority.
pub fn evaluate_native_rain_maintenance_development_v3(
    rows: &[JsonValue],
    population: &JsonValue,
) -> Option<JsonValue> {
    let rows = decode_rain_population(rows, None)?;
    validate_development_population_v3(population, &rows)?;
    // keep the annual historical screen disjoint from prospective receipts
    if !rows
        .iter()
        .all(|row| row.base.evidence_class == "historical_development")
    {
        return None;
    }
    development_report(&rows, Some(text(population, "populationSha256")?))
}

/// Decode and physically validate one complete unique rain population.
fn decode_rain_population(
    values: &[JsonValue],
    historical_row_count: Option<usize>,
) -> Option<Vec<RainRow>> {
    let calendar = NativePredecessorModelCalendar::new()?;
    let rows = values
        .iter()
        .map(|value| decode_rain_row(value, &calendar))
        .collect::<Option<Vec<_>>>()?;
    let base = base_refs(&rows);
    if !native_maintenance_policy_population(&base, NativeMaintenanceCaptureFamily::Rain) {
        return None;
    }
    let historical = rows
        .iter()
        .all(|row| row.base.evidence_class == "historical_development");
    // preserve the frozen original population while allowing V3's proved annual subset
    if historical && historical_row_count.is_some_and(|count| rows.len() != count) {
        return None;
    }
    Some(rows)
}

/// Admit one rain row's complete physical control and probability fields.
fn decode_rain_row(
    value: &JsonValue,
    calendar: &NativePredecessorModelCalendar,
) -> Option<RainRow> {
    let base = decode_native_maintenance_policy_row(value, calendar)?;
    (base.family == NativeMaintenanceCaptureFamily::Rain).then_some(())?;
    let controls = [
        number(value, "persistencePrediction")?,
        number(value, "recentVolumeScalePrediction")?,
        number(value, "sameWindowVolumeScalePrediction")?,
        number(value, "unchangedOrdinalPrediction")?,
        number(value, "volumeScalePrediction")?,
    ];
    let run_key = text(value, "runKey")?;
    let raw_temperature = number(value, "rawTargetHourTemperatureC")?;
    // reject negative controls, cold rows, empty runs, and malformed probability heads
    if run_key.is_empty() || controls.iter().any(|value| *value < 0.0) || raw_temperature <= 2.0 {
        return None;
    }
    Some(RainRow {
        base,
        candidate_probability: probability(value, "candidateProbability")?,
        incumbent_probability: probability(value, "incumbentProbability")?,
        native_source_probability: probability(value, "nativeSourceProbability")?,
        persistence_prediction: controls[0],
        recent_volume_scale_prediction: controls[1],
        same_window_volume_scale_prediction: controls[2],
        unchanged_ordinal_prediction: controls[3],
        volume_scale_prediction: controls[4],
        run_key,
    })
}

/// Decode exactly three ordered finite unit-interval probability heads.
fn probability(value: &JsonValue, key: &str) -> Option<RainProbability> {
    let JsonValue::Object(fields) = field(value, key)? else {
        return None;
    };
    // preserve Object.keys order as well as the closed field population
    if fields.len() != PROBABILITY_KEYS.len()
        || !fields
            .iter()
            .zip(PROBABILITY_KEYS)
            .all(|((actual, _), expected)| actual.iter().copied().eq(expected.encode_utf16()))
    {
        return None;
    }
    let values = PROBABILITY_KEYS.map(|name| number(&JsonValue::Object(fields.clone()), name));
    let [Some(at_least_0_1), Some(at_least_1_0), Some(at_least_2_5)] = values else {
        return None;
    };
    // reject probabilities outside the source unit interval
    if [at_least_0_1, at_least_1_0, at_least_2_5]
        .iter()
        .any(|value| !(0.0..=1.0).contains(value))
    {
        return None;
    }
    Some(RainProbability {
        at_least_0_1,
        at_least_1_0,
        at_least_2_5,
    })
}

/// Derive all 49 stable rain gates in their frozen manifest order.
fn rain_stable_gates(rows: &[RainRow]) -> Option<Vec<Gate>> {
    let mut gates = Vec::with_capacity(STABLE_GATE_IDS.len());
    let overall = rain_metrics(rows);
    push_gate(
        &mut gates,
        "maeImprovesFivePercent",
        overall.candidate_mae <= overall.raw_mae * 0.95,
        number_or_null(Some(overall.candidate_mae)),
        "candidate mae <= raw mae * 0.95",
        false,
    );
    push_gate(
        &mut gates,
        "beatsVolumeScale",
        overall.candidate_mae <= control_mae(rows, |row| row.volume_scale_prediction),
        number_or_null(Some(overall.candidate_mae)),
        "candidate mae <= volume-scale mae",
        false,
    );
    push_gate(
        &mut gates,
        "beatsPersistence",
        overall.candidate_mae <= control_mae(rows, |row| row.persistence_prediction),
        number_or_null(Some(overall.candidate_mae)),
        "candidate mae <= persistence mae",
        false,
    );
    push_gate(
        &mut gates,
        "rmseNoWorse",
        overall.candidate_rmse <= overall.raw_rmse,
        number_or_null(Some(overall.candidate_rmse)),
        "candidate rmse <= raw rmse",
        false,
    );
    push_gate(
        &mut gates,
        "wetIntensityNoWorse",
        matches!((overall.wet_candidate_mae, overall.wet_raw_mae), (Some(candidate), Some(raw)) if candidate <= raw),
        number_or_null(overall.wet_candidate_mae),
        "wet candidate mae <= raw",
        false,
    );
    push_gate(
        &mut gates,
        "heavyIntensityBounded",
        matches!((overall.heavy_candidate_mae, overall.heavy_raw_mae), (Some(candidate), Some(raw)) if candidate <= raw * 1.05),
        number_or_null(overall.heavy_candidate_mae),
        "heavy candidate mae <= raw * 1.05",
        false,
    );
    push_gate(
        &mut gates,
        "annualVolumeBalanced",
        overall
            .volume_ratio
            .is_some_and(|ratio| (0.8..=1.2).contains(&ratio)),
        number_or_null(overall.volume_ratio),
        "volume ratio within 0.8..1.2",
        false,
    );
    push_gate(
        &mut gates,
        "overallSupport",
        support_pass(rows, 300, 100, 50),
        string(&support_text(rows)),
        "dates/wet dates/heavy hours >= 300/100/50",
        true,
    );
    let seasons = season_slices(rows);
    push_gate(
        &mut gates,
        "allSeasonsPresent",
        seasons.iter().all(|(_, selected)| !selected.is_empty()),
        JsonValue::Number(
            seasons
                .iter()
                .filter(|(_, selected)| !selected.is_empty())
                .count() as f64,
        ),
        "all four seasons present",
        true,
    );
    let leads = lead_slices(rows);
    push_gate(
        &mut gates,
        "allLeadBandsPresent",
        leads.iter().all(|(_, selected)| !selected.is_empty()),
        JsonValue::Number(
            leads
                .iter()
                .filter(|(_, selected)| !selected.is_empty())
                .count() as f64,
        ),
        "all three lead bands present",
        true,
    );
    push_gate(
        &mut gates,
        "rawHeavyAmountsUnchanged",
        rows.iter()
            .filter(|row| row.base.native_source_prediction >= 1.0)
            .all(|row| row.base.candidate_prediction == row.base.native_source_prediction),
        JsonValue::Null,
        "candidate preserves raw amounts >= 1",
        false,
    );
    push_gate(
        &mut gates,
        "rawWetCallsPreserved",
        rows.iter()
            .filter(|row| row.base.native_source_prediction >= 0.1)
            .all(|row| row.base.candidate_prediction >= 0.1),
        JsonValue::Null,
        "candidate preserves raw wet calls",
        false,
    );
    // retain the original three event-safety thresholds
    for threshold in RAIN_THRESHOLDS {
        let candidate = event_metrics(rows, |row| row.base.candidate_prediction, threshold);
        let raw = event_metrics(rows, |row| row.base.native_source_prediction, threshold);
        push_gate(
            &mut gates,
            &format!("event{}Safety", threshold_text(threshold)),
            event_safety(candidate, raw),
            number_or_null(candidate.csi),
            "pod nondecrease, csi delta >= -0.01, far delta <= 0.05",
            false,
        );
    }
    // retain each season's support, scalar, intensity, and occurrence gates
    for (season, selected) in seasons {
        let metrics = rain_metrics(&selected);
        let raw_events = event_metrics(&selected, |row| row.base.native_source_prediction, 0.1);
        let candidate_events = event_metrics(&selected, |row| row.base.candidate_prediction, 0.1);
        push_gate(
            &mut gates,
            &format!("season{season}Support"),
            support_pass(&selected, 60, 10, 5),
            string(&support_text(&selected)),
            "dates/wet dates/heavy hours >= 60/10/5",
            true,
        );
        push_gate(
            &mut gates,
            &format!("season{season}Volume"),
            metrics
                .volume_ratio
                .is_some_and(|ratio| (0.8..=1.2).contains(&ratio)),
            number_or_null(metrics.volume_ratio),
            "season volume ratio within 0.8..1.2",
            false,
        );
        push_gate(
            &mut gates,
            &format!("season{season}Mae"),
            metrics.candidate_mae <= metrics.raw_mae * 1.10,
            number_or_null(Some(metrics.candidate_mae)),
            "season candidate mae <= raw * 1.10",
            false,
        );
        let wet_heavy = matches!((metrics.wet_candidate_mae, metrics.wet_raw_mae, metrics.heavy_candidate_mae, metrics.heavy_raw_mae), (Some(wet_candidate), Some(wet_raw), Some(heavy_candidate), Some(heavy_raw)) if wet_candidate <= wet_raw && heavy_candidate <= heavy_raw * 1.05);
        push_gate(
            &mut gates,
            &format!("season{season}WetHeavy"),
            wet_heavy,
            number_or_null(metrics.wet_candidate_mae),
            "season wet no worse and heavy <= raw * 1.05",
            false,
        );
        push_gate(
            &mut gates,
            &format!("season{season}Detection"),
            event_safety(candidate_events, raw_events),
            number_or_null(candidate_events.csi),
            "season pod nondecrease, csi delta >= -0.01, far delta <= 0.05",
            false,
        );
    }
    // retain each operational lead's scalar and support gates
    for (lead, selected) in leads {
        let metrics = rain_metrics(&selected);
        push_gate(
            &mut gates,
            &format!("lead{lead}Mae"),
            metrics.candidate_mae <= metrics.raw_mae * 1.05,
            number_or_null(Some(metrics.candidate_mae)),
            "lead candidate mae <= raw * 1.05",
            false,
        );
        push_gate(
            &mut gates,
            &format!("lead{lead}Support"),
            support_pass(&selected, 0, 20, 5),
            string(&support_text(&selected)),
            "wet dates/heavy hours >= 20/5",
            true,
        );
    }
    // retain complete same-run sliding accumulation gates
    for hours in ACCUMULATION_HOURS {
        let accumulation = rain_accumulation(rows, hours);
        push_gate(
            &mut gates,
            &format!("accumulation{hours}Mae"),
            accumulation.windows > 0 && accumulation.candidate_mae <= accumulation.raw_mae,
            number_or_null(Some(accumulation.candidate_mae)),
            "complete-window candidate mae <= raw",
            false,
        );
    }
    let ordinal_mae = control_mae(rows, |row| row.unchanged_ordinal_prediction);
    push_gate(
        &mut gates,
        "beatsSameWindowVolumeScale",
        overall.candidate_mae <= control_mae(rows, |row| row.same_window_volume_scale_prediction),
        number_or_null(Some(overall.candidate_mae)),
        "candidate mae <= same-window scale mae",
        false,
    );
    push_gate(
        &mut gates,
        "beatsRecentVolumeScale",
        overall.candidate_mae <= control_mae(rows, |row| row.recent_volume_scale_prediction),
        number_or_null(Some(overall.candidate_mae)),
        "candidate mae <= recent scale mae",
        false,
    );
    push_gate(
        &mut gates,
        "beatsUnchangedOrdinal",
        overall.candidate_mae < ordinal_mae - 1e-12,
        number_or_null(Some(overall.candidate_mae)),
        "candidate mae < unchanged ordinal mae - 1e-12",
        false,
    );
    let candidate_deviation = seasonal_volume_deviation(rows, |row| row.base.candidate_prediction);
    let ordinal_deviation = seasonal_volume_deviation(rows, |row| row.unchanged_ordinal_prediction);
    push_gate(
        &mut gates,
        "seasonalBalanceImproves",
        candidate_deviation < ordinal_deviation - 1e-12,
        number_or_null(Some(candidate_deviation)),
        "candidate seasonal deviation improves unchanged ordinal",
        false,
    );
    push_gate(
        &mut gates,
        "heavySkillRetained",
        heavy_skill_retained(rows),
        number_or_null(overall.heavy_candidate_mae),
        "heavy mae and 1.0/2.5 detection retain ordinal skill",
        false,
    );
    // fail closed if the source manifest or order was not reproduced exactly
    if gates.len() != STABLE_GATE_IDS.len()
        || gates
            .iter()
            .zip(STABLE_GATE_IDS)
            .any(|(gate, id)| gate.id != id)
    {
        return None;
    }
    Some(gates)
}

/// Append one stable gate with source pending-on-support semantics.
fn push_gate(
    gates: &mut Vec<Gate>,
    id: &str,
    passing: bool,
    actual: JsonValue,
    criterion: &str,
    pending_on_failure: bool,
) {
    gates.push(if pending_on_failure {
        support_boolean(id, passing, actual, criterion)
    } else {
        boolean_gate(id, passing, actual, criterion)
    });
}

/// Summarize date-balanced scalar rain loss and volume metrics.
fn rain_metrics(rows: &[RainRow]) -> RainMetrics {
    let weights = date_balanced_weights(rows);
    let candidate_mae = weighted_sum(rows, &weights, |row| {
        (row.base.candidate_prediction - row.base.target).abs()
    });
    let raw_mae = weighted_sum(rows, &weights, |row| {
        (row.base.native_source_prediction - row.base.target).abs()
    });
    let observed = weighted_sum(rows, &weights, |row| row.base.target);
    let forecast = weighted_sum(rows, &weights, |row| row.base.candidate_prediction);
    RainMetrics {
        candidate_mae,
        candidate_rmse: weighted_sum(rows, &weights, |row| {
            (row.base.candidate_prediction - row.base.target).powi(2)
        })
        .sqrt(),
        heavy_candidate_mae: conditional_mae(rows, |row| row.base.candidate_prediction, 2.5),
        heavy_raw_mae: conditional_mae(rows, |row| row.base.native_source_prediction, 2.5),
        raw_mae,
        raw_rmse: weighted_sum(rows, &weights, |row| {
            (row.base.native_source_prediction - row.base.target).powi(2)
        })
        .sqrt(),
        volume_ratio: (observed != 0.0).then_some(forecast / observed),
        wet_candidate_mae: conditional_mae(rows, |row| row.base.candidate_prediction, 0.1),
        wet_raw_mae: conditional_mae(rows, |row| row.base.native_source_prediction, 0.1),
    }
}

/// Assign equal local-date mass and then equal row mass within each date.
fn date_balanced_weights(rows: &[RainRow]) -> Vec<f64> {
    let mut counts = HashMap::<&str, usize>::new();
    // count each date without changing first-row order
    for row in rows {
        *counts.entry(&row.base.local_date).or_default() += 1;
    }
    rows.iter()
        .map(|row| 1.0 / counts.len() as f64 / counts[row.base.local_date.as_str()] as f64)
        .collect()
}

/// Sum one weighted row projection in original row order.
fn weighted_sum(rows: &[RainRow], weights: &[f64], value: impl Fn(&RainRow) -> f64) -> f64 {
    rows.iter()
        .zip(weights)
        .fold(0.0, |sum, (row, weight)| sum + value(row) * weight)
}

/// Calculate one conditional date-balanced rain MAE.
fn conditional_mae(
    rows: &[RainRow],
    prediction: impl Fn(&RainRow) -> f64,
    threshold: f64,
) -> Option<f64> {
    let selected = rows
        .iter()
        .filter(|row| row.base.target >= threshold)
        .cloned()
        .collect::<Vec<_>>();
    // preserve unsupported event strata as null
    if selected.is_empty() {
        return None;
    }
    let weights = date_balanced_weights(&selected);
    Some(weighted_sum(&selected, &weights, |row| {
        (prediction(row) - row.base.target).abs()
    }))
}

/// Calculate one control MAE over the identical date-balanced population.
fn control_mae(rows: &[RainRow], prediction: impl Fn(&RainRow) -> f64) -> f64 {
    let weights = date_balanced_weights(rows);
    weighted_sum(rows, &weights, |row| {
        (prediction(row) - row.base.target).abs()
    })
}

/// Count unique dates and event hours for support gates.
fn support(rows: &[RainRow]) -> RainSupport {
    RainSupport {
        dates: rows
            .iter()
            .map(|row| row.base.local_date.as_str())
            .collect::<HashSet<_>>()
            .len(),
        heavy_hours: rows
            .iter()
            .filter(|row| row.base.target >= 2.5)
            .map(|row| row.base.valid_at.as_str())
            .collect::<HashSet<_>>()
            .len(),
        wet_dates: rows
            .iter()
            .filter(|row| row.base.target >= 0.1)
            .map(|row| row.base.local_date.as_str())
            .collect::<HashSet<_>>()
            .len(),
    }
}

/// Compare rain support with one fixed boundary.
fn support_pass(rows: &[RainRow], dates: usize, wet_dates: usize, heavy_hours: usize) -> bool {
    let support = support(rows);
    support.dates >= dates && support.wet_dates >= wet_dates && support.heavy_hours >= heavy_hours
}

/// Serialize one source rain-support diagnostic.
fn support_text(rows: &[RainRow]) -> String {
    let support = support(rows);
    format!(
        "{}/{}/{}",
        support.dates, support.wet_dates, support.heavy_hours
    )
}

/// Calculate deterministic occurrence diagnostics.
fn event_metrics(
    rows: &[RainRow],
    prediction: impl Fn(&RainRow) -> f64,
    threshold: f64,
) -> EventMetrics {
    let mut hits = 0usize;
    let mut misses = 0usize;
    let mut false_alarms = 0usize;
    // count the same paired occurrence rows
    for row in rows {
        let observed = row.base.target >= threshold;
        let called = prediction(row) >= threshold;
        hits += usize::from(observed && called);
        misses += usize::from(observed && !called);
        false_alarms += usize::from(!observed && called);
    }
    EventMetrics {
        csi: ratio(hits, hits + misses + false_alarms),
        far: ratio(false_alarms, hits + false_alarms),
        pod: ratio(hits, hits + misses),
    }
}

/// Return one ratio or source null for a zero denominator.
fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator != 0).then_some(numerator as f64 / denominator as f64)
}

/// Compare candidate event safety with one baseline.
fn event_safety(candidate: EventMetrics, baseline: EventMetrics) -> bool {
    let (Some(candidate_pod), Some(candidate_csi), Some(candidate_far)) =
        (candidate.pod, candidate.csi, candidate.far)
    else {
        return false;
    };
    let (Some(baseline_pod), Some(baseline_csi), Some(baseline_far)) =
        (baseline.pod, baseline.csi, baseline.far)
    else {
        return false;
    };
    candidate_pod >= baseline_pod - 1e-12
        && candidate_csi >= baseline_csi - 0.01
        && candidate_far <= baseline_far + 0.05
}

/// Enumerate exact rain season slices.
fn season_slices(rows: &[RainRow]) -> Vec<(&'static str, Vec<RainRow>)> {
    RAIN_SEASONS
        .iter()
        .map(|(label, season)| {
            (
                *label,
                rows.iter()
                    .filter(|row| row.base.season == *season)
                    .cloned()
                    .collect(),
            )
        })
        .collect()
}

/// Enumerate exact rain operational lead slices.
fn lead_slices(rows: &[RainRow]) -> Vec<(&'static str, Vec<RainRow>)> {
    RAIN_LEADS
        .iter()
        .map(|(label, minimum, maximum)| {
            (
                *label,
                rows.iter()
                    .filter(|row| (*minimum..=*maximum).contains(&row.base.horizon_hours))
                    .cloned()
                    .collect(),
            )
        })
        .collect()
}

/// Score complete same-run rain accumulation windows.
fn rain_accumulation(rows: &[RainRow], hours: usize) -> RainAccumulation {
    let mut runs = Vec::<(String, Vec<&RainRow>)>::new();
    let mut positions = HashMap::<&str, usize>::new();
    // partition rows while preserving first-run insertion order
    for row in rows {
        let index = *positions.entry(&row.run_key).or_insert_with(|| {
            runs.push((row.run_key.clone(), Vec::new()));
            runs.len() - 1
        });
        runs[index].1.push(row);
    }
    let mut candidate_errors = Vec::new();
    let mut raw_errors = Vec::new();
    // scan each source run independently
    for (_, mut run) in runs {
        run.sort_by(|left, right| left.base.valid_at.cmp(&right.base.valid_at));
        // source emits no windows when a run is shorter than the requested span
        if run.len() < hours {
            continue;
        }
        // retain every complete consecutive sliding window
        for start in 0..=run.len() - hours {
            let window = &run[start..start + hours];
            let Some(first) =
                NativeWirePrimitives.parse_explicit_utc_milliseconds(&window[0].base.valid_at)
            else {
                continue;
            };
            let complete = window.iter().enumerate().all(|(index, row)| {
                NativeWirePrimitives.parse_explicit_utc_milliseconds(&row.base.valid_at)
                    == i64::try_from(index)
                        .ok()
                        .and_then(|index| first.checked_add(index * HOUR_MILLISECONDS))
            });
            // skip incomplete time tiling
            if !complete {
                continue;
            }
            let target = window.iter().map(|row| row.base.target).sum::<f64>();
            let candidate = window
                .iter()
                .map(|row| row.base.candidate_prediction)
                .sum::<f64>();
            let raw = window
                .iter()
                .map(|row| row.base.native_source_prediction)
                .sum::<f64>();
            candidate_errors.push((candidate - target).abs());
            raw_errors.push((raw - target).abs());
        }
    }
    RainAccumulation {
        candidate_mae: mean_or_infinity(&candidate_errors),
        raw_mae: mean_or_infinity(&raw_errors),
        windows: candidate_errors.len(),
    }
}

/// Preserve source positive infinity for an absent complete-window population.
fn mean_or_infinity(values: &[f64]) -> f64 {
    if values.is_empty() {
        f64::INFINITY
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// Calculate the worst four-season volume deviation.
fn seasonal_volume_deviation(rows: &[RainRow], prediction: impl Fn(&RainRow) -> f64) -> f64 {
    season_slices(rows)
        .into_iter()
        .map(|(_, selected)| {
            let weights = date_balanced_weights(&selected);
            let observed = weighted_sum(&selected, &weights, |row| row.base.target);
            // keep dry or absent seasons maximally unsupported
            if selected.is_empty() || observed == 0.0 {
                return f64::INFINITY;
            }
            let forecast = weighted_sum(&selected, &weights, &prediction);
            (forecast / observed - 1.0).abs()
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Preserve unchanged-ordinal heavy amount and occurrence skill.
fn heavy_skill_retained(rows: &[RainRow]) -> bool {
    let candidate = conditional_mae(rows, |row| row.base.candidate_prediction, 2.5);
    let ordinal = conditional_mae(rows, |row| row.unchanged_ordinal_prediction, 2.5);
    // reject absent or harmed heavy amount support
    if !matches!((candidate, ordinal), (Some(candidate), Some(ordinal)) if candidate <= ordinal + 1e-12)
    {
        return false;
    }
    // retain both fixed heavy occurrence definitions
    for threshold in [1.0, 2.5] {
        let candidate = event_metrics(rows, |row| row.base.candidate_prediction, threshold);
        let ordinal = event_metrics(rows, |row| row.unchanged_ordinal_prediction, threshold);
        if !event_safety(candidate, ordinal) {
            return false;
        }
    }
    true
}

/// Enumerate rain Best Match nonharm slices in source order.
fn rain_best_match_slices(rows: &[RainRow]) -> Vec<(String, Vec<RainRow>)> {
    season_slices(rows)
        .into_iter()
        .map(|(key, selected)| (format!("season:{key}"), selected))
        .chain(
            lead_slices(rows)
                .into_iter()
                .map(|(key, selected)| (format!("lead:{key}"), selected)),
        )
        .collect()
}

/// Require candidate Brier noninferiority to incumbent and native source.
fn rain_brier_gate(rows: &[RainRow]) -> Gate {
    let weights = date_balanced_weights(rows);
    let mut candidate = 0.0;
    let mut incumbent = 0.0;
    let mut source = 0.0;
    // score all three probability heads on identical rows
    for (row, weight) in rows.iter().zip(weights) {
        let weight = weight / RAIN_THRESHOLDS.len() as f64;
        for threshold in RAIN_THRESHOLDS {
            let observed = if row.base.target >= threshold {
                1.0
            } else {
                0.0
            };
            candidate += weight * (row.candidate_probability.at(threshold) - observed).powi(2);
            incumbent += weight * (row.incumbent_probability.at(threshold) - observed).powi(2);
            source += weight * (row.native_source_probability.at(threshold) - observed).powi(2);
        }
    }
    boolean_gate(
        "probability_brier_nonharm",
        candidate <= incumbent && candidate <= source,
        JsonValue::Number(candidate),
        "candidate Brier <= incumbent and native source",
    )
}

/// Require monotonically nested candidate probabilities.
fn rain_nesting_gate(rows: &[RainRow]) -> Gate {
    let violations = rows
        .iter()
        .filter(|row| {
            row.candidate_probability.at_least_0_1 < row.candidate_probability.at_least_1_0
                || row.candidate_probability.at_least_1_0 < row.candidate_probability.at_least_2_5
        })
        .count();
    boolean_gate(
        "probability_nesting",
        violations == 0,
        JsonValue::Number(violations as f64),
        "p2.5 <= p1.0 <= p0.1",
    )
}

/// Require nonzero probability support for each projected amount category.
fn rain_coherence_gate(rows: &[RainRow]) -> Gate {
    let violations = rows
        .iter()
        .filter(|row| {
            (row.base.candidate_prediction >= 0.1 && row.candidate_probability.at_least_0_1 == 0.0)
                || (row.base.candidate_prediction >= 1.0
                    && row.candidate_probability.at_least_1_0 == 0.0)
                || (row.base.candidate_prediction >= 2.5
                    && row.candidate_probability.at_least_2_5 == 0.0)
        })
        .count();
    boolean_gate(
        "amount_probability_coherence",
        violations == 0,
        JsonValue::Number(violations as f64),
        "projected categories retain nonzero occurrence probability",
    )
}

/// Build one promotion skill gate with exact uncertainty text.
fn skill_gate(id: &str, comparison: Option<&NativeMaintenancePairedComparison>) -> Option<Gate> {
    let Some(comparison) = comparison else {
        return Some(pending_gate(id, "skill >= 0.02 and lower skill > 0"));
    };
    Some(boolean_gate(
        id,
        comparison.skill >= 0.02 && comparison.lower_skill > 0.0,
        string(&format!(
            "{}:{}",
            number_text(comparison.skill)?,
            number_text(comparison.lower_skill)?
        )),
        "skill >= 0.02 and lower skill > 0",
    ))
}

/// Build one upper-bound noninferiority gate.
fn difference_gate(
    id: &str,
    comparison: Option<&NativeMaintenancePairedComparison>,
    maximum: f64,
) -> Gate {
    let criterion = format!(
        "difference upper <= {}",
        number_text(maximum).unwrap_or_default()
    );
    let Some(comparison) = comparison else {
        return pending_gate(id, &criterion);
    };
    boolean_gate(
        id,
        comparison.difference_upper <= maximum,
        JsonValue::Number(comparison.difference_upper),
        &criterion,
    )
}

/// Build one conjunctive rain regression comparison gate.
fn regression_comparison_gate(
    id: &str,
    comparison: Option<&NativeMaintenancePairedComparison>,
) -> Option<Gate> {
    let Some(comparison) = comparison else {
        return Some(pending_gate(
            id,
            "mae >= comparator * 1.05 and difference lower > 0",
        ));
    };
    Some(boolean_gate(
        id,
        comparison.candidate_mae >= comparison.comparator_mae * 1.05
            && comparison.difference_lower > 0.0,
        string(&format!(
            "{}:{}",
            number_text(comparison.candidate_mae)?,
            number_text(comparison.difference_lower)?
        )),
        "mae >= comparator * 1.05 and difference lower > 0",
    ))
}

/// Preserve unavailable bootstrap comparisons as explicit null diagnostics.
fn comparison_or_null(
    rows: &[&NativeMaintenancePolicyRow],
    comparator: NativeMaintenancePolicyComparator,
) -> Option<Option<NativeMaintenancePairedComparison>> {
    let dates = rows
        .iter()
        .map(|row| row.local_date.as_str())
        .collect::<HashSet<_>>()
        .len();
    if dates < 7 {
        Some(None)
    } else {
        Some(Some(native_maintenance_paired_comparison(
            rows, comparator,
        )?))
    }
}

/// Produce the exact nonactionable development report.
fn development_report(rows: &[RainRow], population: Option<String>) -> Option<JsonValue> {
    let gates = rain_stable_gates(rows)?;
    let state = gate_state(&gates);
    let mut fields = vec![("contractVersion", string(POLICY_VERSION))];
    // V3 retains the separately authenticated annual population identity
    if let Some(population) = population {
        fields.push(("developmentPopulationSha256", string(&population)));
    }
    fields.extend([
        ("developmentRows", JsonValue::Number(rows.len() as f64)),
        (
            "gates",
            JsonValue::Array(gates.into_iter().map(gate_value).collect()),
        ),
        ("passed", JsonValue::Boolean(state == GateState::Pass)),
        ("populationSha256", string(&row_population_identity(rows)?)),
        ("productionEligible", JsonValue::Boolean(false)),
        ("state", string(state.wire())),
    ]);
    Some(object(fields))
}

/// Derive one report's conjunction state.
fn gate_state(gates: &[Gate]) -> GateState {
    if gates.iter().any(|gate| gate.state == GateState::Fail) {
        GateState::Fail
    } else if gates.iter().any(|gate| gate.state == GateState::Pending) {
        GateState::Pending
    } else {
        GateState::Pass
    }
}

/// Project one shared gate through the exact source field order.
fn gate_value(gate: Gate) -> JsonValue {
    object(vec![
        ("actual", gate.actual),
        ("criterion", string(&gate.criterion)),
        ("id", string(&gate.id)),
        ("state", string(gate.state.wire())),
    ])
}

/// Hash the exact ordered row-identity population.
fn row_population_identity(rows: &[RainRow]) -> Option<String> {
    canonical_identity(&object(vec![(
        "rows",
        JsonValue::Array(
            rows.iter()
                .map(|row| string(&row.base.row_sha256))
                .collect(),
        ),
    )]))
}

/// Validate the complete annual source population and its eligible row equality.
fn validate_development_population_v3(value: &JsonValue, rows: &[RainRow]) -> Option<()> {
    const KEYS: &[&str] = &[
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
    exact_keys(value, KEYS).then_some(())?;
    (text(value, "contractVersion")? == DEVELOPMENT_POPULATION_VERSION).then_some(())?;
    let wire = NativeWirePrimitives;
    let start_text = text(value, "developmentStartAt")?;
    let end_text = text(value, "developmentEndAt")?;
    let start = wire.parse_explicit_utc_milliseconds(&start_text)?;
    let end = wire.parse_explicit_utc_milliseconds(&end_text)?;
    (start < end && prior_utc_year(&end_text)? == start_text).then_some(())?;
    exact_integer_array(value, "cycleHours", &[0, 6, 12, 18])?;
    exact_integer_array(value, "sourceModelLeadHours", &(9..=31).collect::<Vec<_>>())?;
    exact_integer_array(
        value,
        "operationalHorizonHours",
        &(1..=23).collect::<Vec<_>>(),
    )?;
    let first_run = (start.checked_sub(31 * HOUR_MILLISECONDS)?).div_euclid(6 * HOUR_MILLISECONDS)
        * 6
        * HOUR_MILLISECONDS;
    let mut expected = HashMap::<String, (String, u64, u64, String)>::new();
    let mut run = first_run;
    // enumerate the issuance halo for every target-valid row
    while run <= end.checked_sub(9 * HOUR_MILLISECONDS)? {
        let run_at = wire.format_utc_milliseconds(run)?;
        let issued_at = wire.format_utc_milliseconds(run.checked_add(8 * HOUR_MILLISECONDS)?)?;
        for lead in 9u64..=31 {
            let valid = run.checked_add(i64::try_from(lead).ok()? * HOUR_MILLISECONDS)?;
            // omit only targets outside the half-open annual interval
            if valid < start || valid >= end {
                continue;
            }
            let valid_at = wire.format_utc_milliseconds(valid)?;
            expected.insert(
                format!("{run_at}/{valid_at}"),
                (issued_at.clone(), lead, lead - 8, valid_at),
            );
        }
        run = run.checked_add(6 * HOUR_MILLISECONDS)?;
    }
    let JsonValue::Array(members) = field(value, "sourcePopulation")? else {
        return None;
    };
    let mut keys = HashSet::with_capacity(members.len());
    let mut eligible = 0u64;
    let mut excluded = 0u64;
    let mut member_hashes = Vec::with_capacity(members.len());
    let mut receipt_hashes = BTreeSet::new();
    let mut eligible_keys = Vec::new();
    // validate every archived source identity without reading target values
    for member in members {
        const MEMBER_KEYS: &[&str] = &[
            "issuedAt",
            "key",
            "modelLeadHours",
            "operationalHorizonHours",
            "phaseEligible",
            "sourceMemberSha256",
            "sourceReceiptSha256",
            "targetAvailable",
            "validAt",
        ];
        exact_keys(member, MEMBER_KEYS).then_some(())?;
        let key = text(member, "key")?;
        let geometry = expected.get(&key)?;
        keys.insert(key.clone()).then_some(())?;
        let phase = boolean(member, "phaseEligible")?;
        let target = boolean(member, "targetAvailable")?;
        let source_member = text(member, "sourceMemberSha256")?;
        let source_receipt = text(member, "sourceReceiptSha256")?;
        (text(member, "issuedAt")? == geometry.0
            && integer(member, "modelLeadHours")? == geometry.1
            && integer(member, "operationalHorizonHours")? == geometry.2
            && text(member, "validAt")? == geometry.3
            && valid_hash(&source_member)
            && valid_hash(&source_receipt)
            && (!phase || target))
            .then_some(())?;
        // count phase eligibility and retain its exact key population
        if phase {
            eligible += 1;
            eligible_keys.push(key);
        } else {
            excluded += 1;
        }
        member_hashes.push(source_member);
        receipt_hashes.insert(source_receipt);
    }
    let counts = [
        integer(value, "eligibleRowCount")?,
        integer(value, "excludedColdRowCount")?,
        integer(value, "expectedRowCount")?,
        integer(value, "missingSourceRowCount")?,
        integer(value, "missingTargetRowCount")?,
        integer(value, "observedRowCount")?,
    ];
    member_hashes.sort();
    let receipt_hashes = receipt_hashes.into_iter().collect::<Vec<_>>();
    (counts[0] == eligible
        && counts[1] == excluded
        && counts[2] == expected.len() as u64
        && counts[3] == 0
        && counts[4] == 0
        && counts[5] == expected.len() as u64
        && members.len() == expected.len()
        && keys.len() == expected.len()
        && eligible + excluded == expected.len() as u64
        && text(value, "populationMemberRootSha256")?
            == canonical_identity(&JsonValue::Array(
                member_hashes.iter().map(|hash| string(hash)).collect(),
            ))?
        && text(value, "populationReceiptRootSha256")?
            == canonical_identity(&JsonValue::Array(
                receipt_hashes.iter().map(|hash| string(hash)).collect(),
            ))?
        && text(value, "populationSha256")?
            == canonical_identity(&JsonValue::Array(members.clone()))?)
    .then_some(())?;
    eligible_keys.sort();
    let mut row_keys = rows
        .iter()
        .map(|row| row.base.key.clone())
        .collect::<Vec<_>>();
    row_keys.sort();
    (eligible_keys == row_keys).then_some(())
}

/// Reproduce JavaScript Date.setUTCFullYear for one canonical positive four-digit year.
fn prior_utc_year(value: &str) -> Option<String> {
    let year: u16 = value.get(..4)?.parse().ok()?;
    (year > 0).then_some(())?;
    let suffix = value.get(4..)?;
    let direct = format!("{:04}{suffix}", year - 1);
    // ordinary month/day pairs retain their exact UTC fields
    if NativeWirePrimitives
        .parse_explicit_utc_milliseconds(&direct)
        .is_some()
    {
        return Some(direct);
    }
    // February 29 rolls into March 1 when the preceding year is not leap
    (value.get(4..10)? == "-02-29")
        .then(|| format!("{:04}-03-01{}", year - 1, value.get(10..).unwrap_or("")))
}

/// Require one exact integer array.
fn exact_integer_array(value: &JsonValue, key: &str, expected: &[u64]) -> Option<()> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    (values.len() == expected.len()
        && values.iter().zip(expected).all(|(value, expected)| {
            matches!(value, JsonValue::Number(number) if *number == *expected as f64)
        }))
    .then_some(())
}

/// Decode one nonnegative JavaScript-safe integer field.
fn integer(value: &JsonValue, key: &str) -> Option<u64> {
    let JsonValue::Number(number) = field(value, key)? else {
        return None;
    };
    (number.is_finite()
        && *number >= 0.0
        && *number <= MAXIMUM_SAFE_INTEGER as f64
        && number.fract() == 0.0)
        .then_some(*number as u64)
}

/// Decode one exact JSON boolean field.
fn boolean(value: &JsonValue, key: &str) -> Option<bool> {
    let JsonValue::Boolean(value) = field(value, key)? else {
        return None;
    };
    Some(*value)
}

/// Hash one recursively sorted LF-terminated source value.
fn canonical_identity(value: &JsonValue) -> Option<String> {
    let mut wire = NativeWirePrimitives;
    let bytes = wire.encode_canonical_json(value)?;
    MaintenanceCanonicalJson::sha256(&mut wire, &bytes).map(|hash| hash.as_str().to_owned())
}

/// Convert nullable or nonfinite source numbers to their JSON representation.
fn number_or_null(value: Option<f64>) -> JsonValue {
    value
        .filter(|value| value.is_finite())
        .map_or(JsonValue::Null, JsonValue::Number)
}

/// Render one fixed event threshold exactly like toFixed(1).
fn threshold_text(value: f64) -> String {
    format!("{value:.1}")
}

/// Borrow every shared policy row without changing order.
fn base_refs(rows: &[RainRow]) -> Vec<&NativeMaintenancePolicyRow> {
    rows.iter().map(|row| &row.base).collect()
}

#[cfg(test)]
#[path = "native_maintenance_policy_rain_test.rs"]
mod tests;
