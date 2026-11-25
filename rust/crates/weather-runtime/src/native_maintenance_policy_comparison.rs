//! Date-balanced maintenance model diagnostics using the original deterministic bootstrap.

use crate::{
    NativeMaintenanceCaptureFamily, NativePredecessorModelCalendar, NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_retirement_files_v2::format_digest,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use weather_api::create_wind_moving_block_start_plan;
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

pub const MAINTENANCE_EVALUATION_ROW_VERSION: &str = "maintenance-evaluation-row/v1";

const BASE_ROW_KEYS: &[&str] = &[
    "actualBestMatchPrediction",
    "applied",
    "candidatePrediction",
    "contractVersion",
    "evidenceClass",
    "family",
    "farmTarget",
    "firstEdgeCommittedAt",
    "horizonHours",
    "incumbentPrediction",
    "key",
    "localDate",
    "nativeSourcePrediction",
    "nearestThree",
    "pairKey",
    "provenanceComplete",
    "providerFamily",
    "rowSha256",
    "sourceReceiptAt",
    "sourceRowSha256",
    "stationKey",
    "target",
    "targetRowSha256",
    "validAt",
];
const RAIN_ROW_KEYS: &[&str] = &[
    "candidateProbability",
    "incumbentProbability",
    "nativeSourceProbability",
    "persistencePrediction",
    "rawTargetHourTemperatureC",
    "recentVolumeScalePrediction",
    "runKey",
    "sameWindowVolumeScalePrediction",
    "unchangedOrdinalPrediction",
    "volumeScalePrediction",
];

/// Closed numeric comparator selected by the source report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenancePolicyComparator {
    Candidate,
    Incumbent,
    NativeSource,
    ActualBestMatch,
}
impl NativeMaintenancePolicyComparator {
    /// Return the precise source property name.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Candidate => "candidatePrediction",
            Self::Incumbent => "incumbentPrediction",
            Self::NativeSource => "nativeSourcePrediction",
            Self::ActualBestMatch => "actualBestMatchPrediction",
        }
    }
}

/// Exact admitted evaluation row with cached source calendar classification.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenancePolicyRow {
    pub value: JsonValue,
    pub family: NativeMaintenanceCaptureFamily,
    pub key: String,
    pub local_date: String,
    pub row_sha256: String,
    pub evidence_class: String,
    pub provenance_complete: bool,
    pub applied: bool,
    pub candidate_prediction: f64,
    pub incumbent_prediction: f64,
    pub native_source_prediction: f64,
    pub actual_best_match_prediction: f64,
    pub target: f64,
    pub farm_target: Option<f64>,
    pub horizon_hours: u16,
    pub station_key: Option<String>,
    pub provider_family: Option<String>,
    pub nearest_three: Option<bool>,
    pub pair_key: Option<String>,
    pub valid_at: String,
    pub season: &'static str,
    pub daypart: &'static str,
}
impl NativeMaintenancePolicyRow {
    /// Choose one retained comparator without normalizing its stored model value.
    pub fn prediction(&self, mode: NativeMaintenancePolicyComparator) -> f64 {
        match mode {
            NativeMaintenancePolicyComparator::Candidate => self.candidate_prediction,
            NativeMaintenancePolicyComparator::Incumbent => self.incumbent_prediction,
            NativeMaintenancePolicyComparator::NativeSource => self.native_source_prediction,
            NativeMaintenancePolicyComparator::ActualBestMatch => self.actual_best_match_prediction,
        }
    }
}

/// Admit one actual generated source evaluation row and its canonical content identity.
pub fn decode_native_maintenance_policy_row(
    value: &JsonValue,
    calendar: &NativePredecessorModelCalendar,
) -> Option<NativeMaintenancePolicyRow> {
    let family = match text(value, "family")?.as_str() {
        "temperature" => NativeMaintenanceCaptureFamily::Temperature,
        "wind" => NativeMaintenanceCaptureFamily::Wind,
        "rain" => NativeMaintenanceCaptureFamily::Rain,
        _ => return None,
    };
    let mut keys = BASE_ROW_KEYS.to_vec();
    // rain retains its separate source probability and physical control fields
    if family == NativeMaintenanceCaptureFamily::Rain {
        keys.extend_from_slice(RAIN_ROW_KEYS);
    }
    if !exact_keys(value, &keys)
        || text(value, "contractVersion")? != MAINTENANCE_EVALUATION_ROW_VERSION
    {
        return None;
    }
    let valid_at = text(value, "validAt")?;
    let features = calendar.features(&valid_at).ok()?;
    let local_date = text(value, "localDate")?;
    let key = text(value, "key")?;
    let horizon = number(value, "horizonHours")?;
    let provider_family = nullable_text(value, "providerFamily")?;
    let evidence_class = text(value, "evidenceClass")?;
    let row_sha256 = text(value, "rowSha256")?;
    // retain exact source calendar, physical horizon and immutable provenance identities
    if key.is_empty()
        || local_date != features.local_date
        || horizon.fract() != 0.0
        || !(1.0..=168.0).contains(&horizon)
        || provider_family
            .as_ref()
            .is_some_and(|p| !matches!(p.as_str(), "ambient" | "ecowitt" | "netatmo" | "tempest"))
        || !valid_hash(&text(value, "sourceRowSha256")?)
        || !valid_hash(&text(value, "targetRowSha256")?)
    {
        return None;
    }
    let source = nullable_text(value, "sourceReceiptAt")?;
    let committed = nullable_text(value, "firstEdgeCommittedAt")?;
    let wire = NativeWirePrimitives;
    // distinguish actual prospective receipts from historical training material
    match evidence_class.as_str() {
        "prospective_receipt" => {
            let source = wire.parse_explicit_utc_milliseconds(source.as_deref()?)?;
            let committed = wire.parse_explicit_utc_milliseconds(committed.as_deref()?)?;
            if source > committed || committed >= features.epoch_milliseconds {
                return None;
            }
        }
        "historical_development" if source.is_none() && committed.is_none() => {}
        _ => return None,
    }
    // verify any source-assigned row digest using the exact omitted-self canonical preimage
    if !row_sha256.is_empty() && canonical_without(value, "rowSha256")? != row_sha256 {
        return None;
    }
    let season = weather_contracts::ForecastAdjustmentCoefficientSeasonEnum::WIRE_VALUES
        .iter()
        .find_map(|(name, v)| (*v == features.season).then_some(*name))?;
    let daypart = weather_contracts::ForecastAdjustmentCoefficientDaypartEnum::WIRE_VALUES
        .iter()
        .find_map(|(name, v)| (*v == features.daypart).then_some(*name))?;
    Some(NativeMaintenancePolicyRow {
        value: value.clone(),
        family,
        key,
        local_date,
        row_sha256,
        evidence_class,
        provenance_complete: boolean(value, "provenanceComplete")?,
        applied: boolean(value, "applied")?,
        candidate_prediction: number(value, "candidatePrediction")?,
        incumbent_prediction: number(value, "incumbentPrediction")?,
        native_source_prediction: number(value, "nativeSourcePrediction")?,
        actual_best_match_prediction: number(value, "actualBestMatchPrediction")?,
        target: number(value, "target")?,
        farm_target: nullable_number(value, "farmTarget")?,
        horizon_hours: horizon as u16,
        station_key: nullable_text(value, "stationKey")?,
        provider_family,
        nearest_three: nullable_boolean(value, "nearestThree")?,
        pair_key: nullable_text(value, "pairKey")?,
        valid_at,
        season,
        daypart,
    })
}

/// Construct one exact immutable source row from its original supplied fields.
pub fn create_native_maintenance_policy_row(
    input: &JsonValue,
    calendar: &NativePredecessorModelCalendar,
) -> Option<NativeMaintenancePolicyRow> {
    let JsonValue::Object(fields) = input else {
        return None;
    };
    let mut fields = fields.clone();
    // only the source row constructor owns version and content identity
    if field(input, "contractVersion").is_some() || field(input, "rowSha256").is_some() {
        return None;
    }
    fields.push((
        "contractVersion".encode_utf16().collect(),
        string(MAINTENANCE_EVALUATION_ROW_VERSION),
    ));
    fields.push(("rowSha256".encode_utf16().collect(), string("")));
    let mut value = JsonValue::Object(fields);
    let digest = canonical_without(&value, "rowSha256")?;
    let JsonValue::Object(fields) = &mut value else {
        return None;
    };
    fields
        .iter_mut()
        .find(|(key, _)| key.iter().copied().eq("rowSha256".encode_utf16()))?
        .1 = string(&digest);
    decode_native_maintenance_policy_row(&value, calendar)
}

/// Admit precisely one nonempty unique same-family, same-evidence population.
pub fn native_maintenance_policy_population(
    rows: &[impl std::borrow::Borrow<NativeMaintenancePolicyRow>],
    family: NativeMaintenanceCaptureFamily,
) -> bool {
    let Some(first) = rows.first() else {
        return false;
    };
    let first = first.borrow();
    let mut keys = HashSet::with_capacity(rows.len());
    rows.iter().all(|row| {
        let row = row.borrow();
        row.family == family
            && row.evidence_class == first.evidence_class
            && keys.insert(row.key.as_str())
    })
}

/// Exact bootstrap comparison and its full source report fields.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenancePairedComparison {
    pub candidate_mae: f64,
    pub comparator: NativeMaintenancePolicyComparator,
    pub comparator_mae: f64,
    pub difference: f64,
    pub difference_lower: f64,
    pub difference_upper: f64,
    pub event_count: usize,
    pub lower_skill: f64,
    pub skill: f64,
    pub upper_skill: f64,
}
impl NativeMaintenancePairedComparison {
    /// Emit the original canonical JSON report representation.
    pub fn value(&self) -> JsonValue {
        object(vec![
            ("candidateMae", JsonValue::Number(self.candidate_mae)),
            ("comparator", string(self.comparator.wire())),
            ("comparatorMae", JsonValue::Number(self.comparator_mae)),
            ("difference", JsonValue::Number(self.difference)),
            ("differenceLower", JsonValue::Number(self.difference_lower)),
            ("differenceUpper", JsonValue::Number(self.difference_upper)),
            ("eventCount", JsonValue::Number(self.event_count as f64)),
            ("lowerSkill", JsonValue::Number(self.lower_skill)),
            ("replicates", JsonValue::Number(2000.0)),
            ("skill", JsonValue::Number(self.skill)),
            ("upperSkill", JsonValue::Number(self.upper_skill)),
        ])
    }
}

/// The two acting comparison roots and the separate labelled diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenancePolicyPopulationReport {
    pub actual_best_match_all_rows: Option<NativeMaintenancePairedComparison>,
    pub actual_best_match_applied_only: Option<NativeMaintenancePairedComparison>,
    pub farm_diagnostic_mae: Option<f64>,
    pub incumbent: Option<NativeMaintenancePairedComparison>,
    pub native_source: Option<NativeMaintenancePairedComparison>,
    pub population_sha256: String,
}
impl NativeMaintenancePolicyPopulationReport {
    /// Preserve explicitly unavailable comparisons and diagnostic values.
    pub fn value(&self) -> JsonValue {
        let value = |r: &Option<NativeMaintenancePairedComparison>| {
            r.as_ref()
                .map_or(JsonValue::Null, NativeMaintenancePairedComparison::value)
        };
        object(vec![
            (
                "actualBestMatchAllRows",
                value(&self.actual_best_match_all_rows),
            ),
            (
                "actualBestMatchAppliedOnly",
                value(&self.actual_best_match_applied_only),
            ),
            (
                "farmDiagnosticMae",
                self.farm_diagnostic_mae
                    .map_or(JsonValue::Null, JsonValue::Number),
            ),
            ("incumbent", value(&self.incumbent)),
            ("nativeSource", value(&self.native_source)),
            ("populationSha256", string(&self.population_sha256)),
        ])
    }
}

/// Preserve date-balanced paired losses for one acting population.
pub fn native_maintenance_paired_comparison(
    rows: &[&NativeMaintenancePolicyRow],
    comparator: NativeMaintenancePolicyComparator,
) -> Option<NativeMaintenancePairedComparison> {
    let dates = day_losses(rows, comparator)?;
    compare_groups(&[dates], comparator, rows.len())
}

/// Preserve equal pair weight in the original wind regression population.
pub fn native_maintenance_equal_group_comparison(
    rows: &[&NativeMaintenancePolicyRow],
    keys: &[&str],
    comparator: NativeMaintenancePolicyComparator,
) -> Option<NativeMaintenancePairedComparison> {
    let mut groups = Vec::with_capacity(keys.len());
    // retain group order and source row order inside each independently weighted group
    for key in keys {
        let group = rows
            .iter()
            .copied()
            .filter(|row| row.pair_key.as_deref() == Some(key))
            .collect::<Vec<_>>();
        groups.push(day_losses(&group, comparator)?);
    }
    compare_groups(&groups, comparator, rows.len())
}

/// Summarize the source acting comparators and applied/farm diagnostics.
pub fn native_maintenance_population_report(
    rows: &[&NativeMaintenancePolicyRow],
) -> Option<NativeMaintenancePolicyPopulationReport> {
    use NativeMaintenancePolicyComparator as C;
    let applied = rows
        .iter()
        .copied()
        .filter(|row| row.applied)
        .collect::<Vec<_>>();
    let (sum, count) = rows
        .iter()
        .filter_map(|row| {
            row.farm_target
                .map(|target| (row.candidate_prediction - target).abs())
        })
        .fold((0.0, 0usize), |(sum, count), loss| (sum + loss, count + 1));
    let population = object(vec![(
        "rows",
        JsonValue::Array(rows.iter().map(|row| string(&row.row_sha256)).collect()),
    )]);
    let wire = NativeWirePrimitives;
    let population_sha256 = format_digest(wire.sha256(&wire.encode_sorted_json(&population)?));
    Some(NativeMaintenancePolicyPopulationReport {
        actual_best_match_all_rows: comparison_or_null(rows, C::ActualBestMatch)?,
        actual_best_match_applied_only: comparison_or_null(&applied, C::ActualBestMatch)?,
        farm_diagnostic_mae: (count != 0).then(|| sum / count as f64),
        incumbent: comparison_or_null(rows, C::Incumbent)?,
        native_source: comparison_or_null(rows, C::NativeSource)?,
        population_sha256,
    })
}

/// Count distinct source local dates without normalizing chronology.
pub fn native_maintenance_date_count(rows: &[&NativeMaintenancePolicyRow]) -> usize {
    rows.iter()
        .map(|r| r.local_date.as_str())
        .collect::<HashSet<_>>()
        .len()
}

/// Preserve the source first-occurrence date order for nonharm diagnostic MAE.
pub fn native_maintenance_mean_absolute_error(
    rows: &[&NativeMaintenancePolicyRow],
    comparator: NativeMaintenancePolicyComparator,
) -> Option<f64> {
    if rows.is_empty() {
        return None;
    }
    let mut slots: Vec<(f64, usize)> = Vec::new();
    let mut keys = HashMap::new();
    // accumulate each date in original row order and retain date insertion order
    for row in rows {
        let index = *keys.entry(row.local_date.as_str()).or_insert_with(|| {
            slots.push((0.0, 0));
            slots.len() - 1
        });
        slots[index].0 += (row.prediction(comparator) - row.target).abs();
        slots[index].1 += 1;
    }
    let count = slots.len() as f64;
    Some(
        slots
            .into_iter()
            .fold(0.0, |sum, (loss, rows)| sum + (loss / rows as f64) / count),
    )
}

// preserve the unsupported source comparison as explicit null
fn comparison_or_null(
    rows: &[&NativeMaintenancePolicyRow],
    comparator: NativeMaintenancePolicyComparator,
) -> Option<Option<NativeMaintenancePairedComparison>> {
    if native_maintenance_date_count(rows) < 7 {
        Some(None)
    } else {
        Some(Some(native_maintenance_paired_comparison(
            rows, comparator,
        )?))
    }
}

// accumulate losses in source row order and expose sorted date slots
fn day_losses(
    rows: &[&NativeMaintenancePolicyRow],
    comparator: NativeMaintenancePolicyComparator,
) -> Option<Vec<(f64, f64)>> {
    let mut dates = BTreeMap::<&str, (f64, f64, usize)>::new();
    for row in rows {
        let slot = dates.entry(row.local_date.as_str()).or_default();
        slot.0 += (row.candidate_prediction - row.target).abs();
        slot.1 += (row.prediction(comparator) - row.target).abs();
        slot.2 += 1;
    }
    if dates.len() < 7 {
        return None;
    }
    Some(
        dates
            .into_values()
            .map(|(a, b, count)| (a / count as f64, b / count as f64))
            .collect(),
    )
}

// apply the exact original two thousand replicate non-circular draw stream
fn compare_groups(
    groups: &[Vec<(f64, f64)>],
    comparator: NativeMaintenancePolicyComparator,
    event_count: usize,
) -> Option<NativeMaintenancePairedComparison> {
    let plan =
        create_wind_moving_block_start_plan(&groups.iter().map(Vec::len).collect::<Vec<_>>())
            .ok()?;
    let group_count = groups.len() as f64;
    let (candidate_mae, comparator_mae) = groups.iter().fold((0.0, 0.0), |(a, b), group| {
        let (c, d) = group.iter().fold((0.0, 0.0), |(c, d), (x, y)| {
            (c + x / group.len() as f64, d + y / group.len() as f64)
        });
        (a + c / group_count, b + d / group_count)
    });
    let mut skills = Vec::with_capacity(2000);
    let mut differences = Vec::with_capacity(2000);
    // stream each exact sampled date in original plan order without intermediate date copies
    for replicate in plan {
        let mut candidate = 0.0;
        let mut baseline = 0.0;
        for (group, starts) in groups.iter().zip(replicate) {
            let (mut c, mut d) = (0.0, 0.0);
            let mut count = 0;
            for start in starts {
                for offset in 0..7 {
                    if count == group.len() {
                        break;
                    }
                    let (x, y) = group[start + offset];
                    c += x / group.len() as f64;
                    d += y / group.len() as f64;
                    count += 1;
                }
            }
            if count != group.len() {
                return None;
            }
            candidate += c / group_count;
            baseline += d / group_count;
        }
        skills.push(skill(baseline, candidate)?);
        differences.push(candidate - baseline);
    }
    if skills.len() != 2000 {
        return None;
    }
    skills.sort_by(f64::total_cmp);
    differences.sort_by(f64::total_cmp);
    Some(NativeMaintenancePairedComparison {
        candidate_mae,
        comparator,
        comparator_mae,
        difference: candidate_mae - comparator_mae,
        difference_lower: differences[49],
        difference_upper: differences[1949],
        event_count,
        lower_skill: skills[49],
        skill: skill(comparator_mae, candidate_mae)?,
        upper_skill: skills[1949],
    })
}

// reproduce source paired skill for both zero-baseline cases
fn skill(baseline: f64, candidate: f64) -> Option<f64> {
    if !baseline.is_finite() || !candidate.is_finite() || baseline < 0.0 || candidate < 0.0 {
        return None;
    }
    Some(if baseline == 0.0 {
        if candidate == 0.0 { 0.0 } else { -1.0 }
    } else {
        (baseline - candidate) / baseline
    })
}

// static source canonical object and self-excluded digests
pub(crate) fn canonical_without(value: &JsonValue, key: &str) -> Option<String> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    let copy = JsonValue::Object(
        fields
            .iter()
            .filter(|(name, _)| !name.iter().copied().eq(key.encode_utf16()))
            .cloned()
            .collect(),
    );
    Some(format_digest(
        NativeWirePrimitives.sha256(&NativeWirePrimitives.encode_sorted_json(&copy)?),
    ))
}

pub(crate) fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}
pub(crate) fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

// admit finite numerical and nullable source fields without manufactured values
pub(crate) fn number(value: &JsonValue, key: &str) -> Option<f64> {
    match field(value, key)? {
        JsonValue::Number(n) if n.is_finite() => Some(*n),
        _ => None,
    }
}
fn nullable_number(value: &JsonValue, key: &str) -> Option<Option<f64>> {
    if *field(value, key)? == JsonValue::Null {
        Some(None)
    } else {
        Some(Some(number(value, key)?))
    }
}
fn nullable_text(value: &JsonValue, key: &str) -> Option<Option<String>> {
    if *field(value, key)? == JsonValue::Null {
        Some(None)
    } else {
        Some(Some(text(value, key)?))
    }
}
fn boolean(value: &JsonValue, key: &str) -> Option<bool> {
    match field(value, key)? {
        JsonValue::Boolean(v) => Some(*v),
        _ => None,
    }
}
fn nullable_boolean(value: &JsonValue, key: &str) -> Option<Option<bool>> {
    if *field(value, key)? == JsonValue::Null {
        Some(None)
    } else {
        Some(Some(boolean(value, key)?))
    }
}
