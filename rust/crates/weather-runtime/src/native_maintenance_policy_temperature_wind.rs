//! Exact temperature and wind qualification gate calculations over admitted canonical rows.

use crate::{
    NativeMaintenanceCaptureFamily, NativePredecessorModelCalendar, NativeWirePrimitives,
    maintenance_canonical_json::text,
    native_fit_ascii_collation::{native_fit_collation_admits, native_fit_collation_cmp},
    native_maintenance_policy_comparison::{
        NativeMaintenancePairedComparison, NativeMaintenancePolicyComparator,
        NativeMaintenancePolicyPopulationReport, NativeMaintenancePolicyRow,
        decode_native_maintenance_policy_row, native_maintenance_date_count,
        native_maintenance_equal_group_comparison, native_maintenance_mean_absolute_error,
        native_maintenance_paired_comparison, native_maintenance_policy_population,
        native_maintenance_population_report, object, string,
    },
};
use std::collections::{BTreeMap, HashSet};
use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

const SEASONS: [&str; 4] = ["winter", "spring", "summer", "autumn"];
const DAYPARTS: [&str; 4] = ["night", "morning", "afternoon", "evening"];
const WIND_PAIRS: [&str; 13] = [
    "windGustMps:001-024",
    "windGustMps:025-048",
    "windGustMps:073-096",
    "windGustMps:097-120",
    "windGustMps:121-144",
    "windGustMps:145-168",
    "windSpeedMps:001-024",
    "windSpeedMps:025-048",
    "windSpeedMps:049-072",
    "windSpeedMps:073-096",
    "windSpeedMps:097-120",
    "windSpeedMps:121-144",
    "windSpeedMps:145-168",
];

/// Closed policy kind with its source epoch requirement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenancePolicyKind {
    Promotion,
    Regression,
}

impl NativeMaintenancePolicyKind {
    /// Return the exact original policy mode.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Promotion => "promotion",
            Self::Regression => "regression",
        }
    }
}

/// Exact policy report gate state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GateState {
    Pass,
    Fail,
    Pending,
}

impl GateState {
    /// Obtain the source JSON state spelling.
    pub(crate) const fn wire(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Pending => "pending",
        }
    }
}

/// One exact source gate receipt, including diagnostic actual values.
#[derive(Clone, Debug)]
pub(crate) struct Gate {
    pub(crate) id: String,
    pub(crate) criterion: String,
    pub(crate) actual: JsonValue,
    pub(crate) state: GateState,
}

impl Gate {
    /// Convert a complete gate into its canonical original JSON projection.
    fn value(self) -> JsonValue {
        object(vec![
            ("id", string(&self.id)),
            ("criterion", string(&self.criterion)),
            ("actual", self.actual),
            ("state", string(self.state.wire())),
        ])
    }
}

/// Evaluate a complete original temperature or wind promotion/regression policy and emit the exact report.
pub fn evaluate_native_temperature_wind_maintenance_policy(
    family: NativeMaintenanceCaptureFamily,
    kind: NativeMaintenancePolicyKind,
    rows: &[JsonValue],
    epoch: Option<&JsonValue>,
) -> Option<JsonValue> {
    if (kind == NativeMaintenancePolicyKind::Promotion) != epoch.is_none() {
        return None;
    }
    let calendar = NativePredecessorModelCalendar::new()?;
    let rows = rows
        .iter()
        .map(|row| decode_native_maintenance_policy_row(row, &calendar))
        .collect::<Option<Vec<_>>>()?;
    if !native_maintenance_policy_population(&rows, family) {
        return None;
    }
    let population = rows.iter().collect::<Vec<_>>();
    let report = match family {
        NativeMaintenanceCaptureFamily::Temperature => {
            temperature_policy(&population, kind, epoch)?
        }
        NativeMaintenanceCaptureFamily::Wind => wind_policy(&population, kind, epoch)?,
        NativeMaintenanceCaptureFamily::Rain => return None,
    };
    Some(report)
}

// evaluate both original temperature policy modes over the closed support matrix
fn temperature_policy(
    rows: &[&NativeMaintenancePolicyRow],
    kind: NativeMaintenancePolicyKind,
    epoch: Option<&JsonValue>,
) -> Option<JsonValue> {
    let mut gates = base_evidence_gates(rows);
    let population = native_maintenance_population_report(rows)?;
    match kind {
        NativeMaintenancePolicyKind::Promotion => {
            let slices = temperature_slices(rows);
            // require every acting horizon, season and daypart slice
            for (id, slice) in &slices {
                gates.push(support_gate(id, slice, 60, 1000));
            }
            gates.push(skill_gate(
                "incumbent_skill",
                population.incumbent.as_ref(),
                false,
            )?);
            gates.push(skill_gate(
                "native_source_skill",
                population.native_source.as_ref(),
                false,
            )?);
            gates.push(difference_gate(
                "actual_best_match_margin",
                population.actual_best_match_all_rows.as_ref(),
                false,
            ));
            for (id, slice) in slices.iter().skip(1) {
                gates.push(nonharm_gate(&format!("{id}_nonharm"), slice));
            }
        }
        NativeMaintenancePolicyKind::Regression => {
            gates.push(epoch_gate(epoch?)?);
            gates.push(support_gate("overall_support", rows, 60, 1000));
            gates.push(skill_gate(
                "active_native_source_harm",
                population.native_source.as_ref(),
                true,
            )?);
            gates.push(difference_gate(
                "active_best_match_harm",
                population.actual_best_match_all_rows.as_ref(),
                true,
            ));
        }
    }
    Some(finalize(
        "temperature-maintenance-policy/v2",
        kind,
        population,
        gates,
    ))
}

// evaluate every original wind pair and separately retained harm slice
fn wind_policy(
    rows: &[&NativeMaintenancePolicyRow],
    kind: NativeMaintenancePolicyKind,
    epoch: Option<&JsonValue>,
) -> Option<JsonValue> {
    let actual = rows
        .iter()
        .map(|row| row.pair_key.as_deref())
        .collect::<HashSet<_>>();
    if actual.len() != WIND_PAIRS.len()
        || WIND_PAIRS.iter().any(|key| !actual.contains(&Some(*key)))
    {
        return None;
    }
    let mut gates = base_evidence_gates(rows);
    use NativeMaintenancePolicyComparator as C;
    match kind {
        NativeMaintenancePolicyKind::Promotion => {
            // preserve source pair ordering and exact independent pair evidence
            for pair in WIND_PAIRS {
                let selected = rows
                    .iter()
                    .copied()
                    .filter(|row| row.pair_key.as_deref() == Some(pair))
                    .collect::<Vec<_>>();
                gates.push(support_gate(
                    &format!("pair:{pair}:support"),
                    &selected,
                    60,
                    100,
                ));
                let report = native_maintenance_population_report(&selected)?;
                gates.push(skill_gate(
                    &format!("pair:{pair}:incumbent"),
                    report.incumbent.as_ref(),
                    false,
                )?);
                gates.push(skill_gate(
                    &format!("pair:{pair}:native_source"),
                    report.native_source.as_ref(),
                    false,
                )?);
                gates.push(difference_gate(
                    &format!("pair:{pair}:actual_best_match"),
                    report.actual_best_match_all_rows.as_ref(),
                    false,
                ));
            }
            // retain the four source season support gates in fixed order
            for season in SEASONS {
                let selected = rows
                    .iter()
                    .copied()
                    .filter(|row| row.season == season)
                    .collect::<Vec<_>>();
                gates.push(support_gate(
                    &format!("season:{season}:support"),
                    &selected,
                    60,
                    100,
                ));
            }
            let mut slices = wind_harm_slices(rows);
            // the finite verified native collation profile must cover every original slice key
            if slices
                .iter()
                .any(|(key, _)| !native_fit_collation_admits(key))
            {
                return None;
            }
            slices.sort_by(|(a, _), (b, _)| native_fit_collation_cmp(a, b));
            for (key, selected) in slices {
                if selected.len() >= 100 {
                    let comparison =
                        native_maintenance_paired_comparison(&selected, C::NativeSource)?;
                    let harmed = comparison.skill <= -0.02 && comparison.upper_skill < 0.0;
                    let actual = string(&format!(
                        "{}:{}",
                        number_text(comparison.skill)?,
                        number_text(comparison.upper_skill)?
                    ));
                    gates.push(boolean_gate(
                        &format!("slice:{key}:no_material_harm"),
                        !harmed,
                        actual,
                        "skill > -0.02 or upper skill >= 0",
                    ));
                }
            }
        }
        NativeMaintenancePolicyKind::Regression => {
            gates.push(epoch_gate(epoch?)?);
            for pair in WIND_PAIRS {
                let selected = rows
                    .iter()
                    .copied()
                    .filter(|row| row.pair_key.as_deref() == Some(pair))
                    .collect::<Vec<_>>();
                gates.push(support_gate(
                    &format!("pair:{pair}:support"),
                    &selected,
                    60,
                    100,
                ));
            }
            let native =
                native_maintenance_equal_group_comparison(rows, &WIND_PAIRS, C::NativeSource)?;
            let actual =
                native_maintenance_equal_group_comparison(rows, &WIND_PAIRS, C::ActualBestMatch)?;
            gates.push(skill_gate(
                "equal_pair_native_source_harm",
                Some(&native),
                true,
            )?);
            gates.push(difference_gate(
                "equal_pair_actual_best_match_harm",
                Some(&actual),
                true,
            ));
        }
    }
    Some(finalize(
        "wind-maintenance-policy/v2",
        kind,
        native_maintenance_population_report(rows)?,
        gates,
    ))
}

// construct the exact source support slices with their retained row ordering
fn temperature_slices<'a>(
    rows: &[&'a NativeMaintenancePolicyRow],
) -> Vec<(String, Vec<&'a NativeMaintenancePolicyRow>)> {
    let mut slices = vec![("overall_support".to_owned(), rows.to_vec())];
    for (horizon, min, max) in [("1-6", 1, 6), ("7-12", 7, 12)] {
        slices.push((
            format!("horizon:{horizon}:support"),
            rows.iter()
                .copied()
                .filter(|row| (min..=max).contains(&row.horizon_hours))
                .collect(),
        ));
    }
    for season in SEASONS {
        slices.push((
            format!("season:{season}:support"),
            rows.iter()
                .copied()
                .filter(|row| row.season == season)
                .collect(),
        ));
    }
    for daypart in DAYPARTS {
        slices.push((
            format!("daypart:{daypart}:support"),
            rows.iter()
                .copied()
                .filter(|row| row.daypart == daypart)
                .collect(),
        ));
    }
    slices
}

// populate prespecified station, provider, nearest-three, season and daypart slices
fn wind_harm_slices<'a>(
    rows: &[&'a NativeMaintenancePolicyRow],
) -> Vec<(String, Vec<&'a NativeMaintenancePolicyRow>)> {
    let mut slices = BTreeMap::<String, Vec<&NativeMaintenancePolicyRow>>::new();
    for row in rows {
        let keys = [
            row.station_key.as_ref().map(|key| format!("station:{key}")),
            row.provider_family
                .as_ref()
                .map(|key| format!("provider:{key}")),
            (row.nearest_three == Some(true)).then(|| "nearest-three".to_owned()),
            Some(format!("season:{}", row.season)),
            Some(format!("daypart:{}", row.daypart)),
        ];
        for key in keys.into_iter().flatten() {
            slices.entry(key).or_default().push(row);
        }
    }
    slices.into_iter().collect()
}

/// Build the two source prospective-evidence and provenance receipts.
pub(crate) fn base_evidence_gates(rows: &[&NativeMaintenancePolicyRow]) -> Vec<Gate> {
    let prospective = rows
        .iter()
        .all(|row| row.evidence_class == "prospective_receipt");
    let provenance = rows.iter().all(|row| row.provenance_complete);
    vec![
        support_boolean(
            "prospective_evidence",
            prospective,
            string(if prospective {
                "prospective_receipt"
            } else {
                "historical_development"
            }),
            "all rows are prospective receipts",
        ),
        support_boolean(
            "complete_provenance",
            provenance,
            string(if provenance { "complete" } else { "incomplete" }),
            "all source and target provenance is complete",
        ),
    ]
}

// preserve the exact source support gate count and pending disposition
fn support_gate(
    id: &str,
    rows: &[&NativeMaintenancePolicyRow],
    minimum_dates: usize,
    minimum_rows: usize,
) -> Gate {
    let dates = native_maintenance_date_count(rows);
    let pass = dates >= minimum_dates && rows.len() >= minimum_rows;
    support_boolean(
        id,
        pass,
        string(&format!("{dates}/{}", rows.len())),
        &format!("dates >= {minimum_dates} and rows >= {minimum_rows}"),
    )
}

// preserve source skill gate diagnostics and exact threshold comparison
fn skill_gate(
    id: &str,
    comparison: Option<&NativeMaintenancePairedComparison>,
    regression: bool,
) -> Option<Gate> {
    let criterion = if regression {
        "skill <= -0.02 and upper skill < 0"
    } else {
        "skill >= 0.02 and lower skill > 0"
    };
    let Some(comparison) = comparison else {
        return Some(pending_gate(id, criterion));
    };
    let bound = if regression {
        comparison.upper_skill
    } else {
        comparison.lower_skill
    };
    let passing = if regression {
        comparison.skill <= -0.02 && bound < 0.0
    } else {
        comparison.skill >= 0.02 && bound > 0.0
    };
    Some(boolean_gate(
        id,
        passing,
        string(&format!(
            "{}:{}",
            number_text(comparison.skill)?,
            number_text(bound)?
        )),
        criterion,
    ))
}

// preserve source upper/lower confidence-bound noninferiority comparison
fn difference_gate(
    id: &str,
    comparison: Option<&NativeMaintenancePairedComparison>,
    regression: bool,
) -> Gate {
    let criterion = if regression {
        "difference lower > 0.1"
    } else {
        "difference upper <= 0.1"
    };
    let Some(comparison) = comparison else {
        return pending_gate(id, criterion);
    };
    let actual = if regression {
        comparison.difference_lower
    } else {
        comparison.difference_upper
    };
    boolean_gate(
        id,
        if regression {
            actual > 0.1
        } else {
            actual <= 0.1
        },
        JsonValue::Number(actual),
        criterion,
    )
}

// retain each temperature slice's date-balanced nonharm gate
fn nonharm_gate(id: &str, rows: &[&NativeMaintenancePolicyRow]) -> Gate {
    use NativeMaintenancePolicyComparator as C;
    let values = [
        C::Candidate,
        C::Incumbent,
        C::NativeSource,
        C::ActualBestMatch,
    ]
    .map(|comparator| native_maintenance_mean_absolute_error(rows, comparator));
    let [
        Some(candidate),
        Some(incumbent),
        Some(source),
        Some(best_match),
    ] = values
    else {
        return pending_gate(
            id,
            "candidate no worse than incumbent, source, or Best Match margin",
        );
    };
    boolean_gate(
        id,
        candidate <= incumbent && candidate <= source && candidate <= best_match + 0.1,
        JsonValue::Number(candidate),
        "candidate <= incumbent/source and <= Best Match + 0.1",
    )
}

/// Bind the source original completed epoch and seven-day action delay.
pub(crate) fn epoch_gate(epoch: &JsonValue) -> Option<Gate> {
    let registered = text(epoch, "registeredAt")?;
    let start = text(epoch, "epochStartInclusive")?;
    let end = text(epoch, "epochEndExclusive")?;
    let evaluated = text(epoch, "evaluatedAt")?;
    let parse = |value: &str| NativeWirePrimitives.parse_explicit_utc_milliseconds(value);
    let pass = match (
        parse(&registered),
        parse(&start),
        parse(&end),
        parse(&evaluated),
    ) {
        (Some(registered), Some(start), Some(end), Some(evaluated)) => {
            registered < start
                && start < end
                && end
                    .checked_add(7 * 24 * 60 * 60 * 1000)
                    .is_some_and(|after| evaluated >= after)
        }
        _ => false,
    };
    Some(support_boolean(
        "registered_complete_epoch",
        pass,
        string(&format!("{registered}/{evaluated}")),
        "registered before start and evaluated seven days after close",
    ))
}

/// Create one ordinary fail/pass source gate.
pub(crate) fn boolean_gate(id: &str, passing: bool, actual: JsonValue, criterion: &str) -> Gate {
    Gate {
        id: id.to_owned(),
        criterion: criterion.to_owned(),
        actual,
        state: if passing {
            GateState::Pass
        } else {
            GateState::Fail
        },
    }
}

/// Create one pending source evidence gate while preserving the diagnostic value.
pub(crate) fn support_boolean(id: &str, passing: bool, actual: JsonValue, criterion: &str) -> Gate {
    Gate {
        id: id.to_owned(),
        criterion: criterion.to_owned(),
        actual,
        state: if passing {
            GateState::Pass
        } else {
            GateState::Pending
        },
    }
}

/// Create one unsupported source metric gate.
pub(crate) fn pending_gate(id: &str, criterion: &str) -> Gate {
    Gate {
        id: id.to_owned(),
        criterion: criterion.to_owned(),
        actual: JsonValue::Null,
        state: GateState::Pending,
    }
}

/// Finalize the original conjunction and action without an unsupported pass.
pub(crate) fn finalize(
    version: &str,
    kind: NativeMaintenancePolicyKind,
    population: NativeMaintenancePolicyPopulationReport,
    gates: Vec<Gate>,
) -> JsonValue {
    let state = if gates.iter().any(|gate| gate.state == GateState::Fail) {
        GateState::Fail
    } else if gates.iter().any(|gate| gate.state == GateState::Pending) {
        GateState::Pending
    } else {
        GateState::Pass
    };
    let action = match (state, kind) {
        (GateState::Pending, _) => "pending",
        (GateState::Fail, _) => "retain",
        (GateState::Pass, NativeMaintenancePolicyKind::Promotion) => "promote",
        (GateState::Pass, NativeMaintenancePolicyKind::Regression) => "regress",
    };
    object(vec![
        ("action", string(action)),
        ("contractVersion", string(version)),
        (
            "gates",
            JsonValue::Array(gates.into_iter().map(Gate::value).collect()),
        ),
        ("mode", string(kind.wire())),
        ("population", population.value()),
        ("state", string(state.wire())),
    ])
}

/// Render original JavaScript number text using the accepted native primitive.
pub(crate) fn number_text(value: f64) -> Option<String> {
    let bytes = NativeWirePrimitives.encode_sorted_json(&JsonValue::Number(value))?;
    let text = String::from_utf8(bytes).ok()?;
    Some(text.strip_suffix('\n')?.to_owned())
}

#[cfg(test)]
#[path = "native_maintenance_policy_temperature_wind_test.rs"]
mod tests;
