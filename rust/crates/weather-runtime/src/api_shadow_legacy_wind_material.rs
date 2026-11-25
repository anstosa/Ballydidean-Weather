//! Closed fitted-material validation for the retained legacy wind canary.

use weather_contracts::JsonValue;

use crate::api_shadow_legacy_values::{integer_field, number_field};
use crate::api_shadow_wire_validation::rows;
use crate::maintenance_canonical_json::{exact_keys, field, text};

const METRIC_BAND_KEYS: &[&str] = &["leadBand", "metric"];
const COEFFICIENT_KEYS: &[&str] = &[
    "coefficient",
    "daypart",
    "effectiveEventCount",
    "leadBand",
    "level",
    "metric",
    "month",
    "season",
];
const TRAINING_ENVELOPE_KEYS: &[&str] = &["leadBand", "maximum", "metric", "minimum"];
const LEAD_BANDS: &[&str] = &[
    "001-024", "025-048", "049-072", "073-096", "097-120", "121-144", "145-168",
];

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct MetricBand {
    metric: String,
    lead_band: String,
}

impl MetricBand {
    /// Read one closed wind-only metric and lead-band pair.
    fn read(node: &JsonValue) -> Option<Self> {
        exact_keys(node, METRIC_BAND_KEYS).then_some(())?;
        Self::read_fields(node)
    }

    /// Read the metric-band fields from a larger closed record.
    fn read_fields(node: &JsonValue) -> Option<Self> {
        let metric = text(node, "metric")?;
        let lead_band = text(node, "leadBand")?;
        // retain only the three wind metrics and seven frozen lead bands
        if !matches!(
            metric.as_str(),
            "windDirectionDegrees" | "windGustMps" | "windSpeedMps"
        ) || !LEAD_BANDS.contains(&lead_band.as_str())
        {
            return None;
        }
        Some(Self { metric, lead_band })
    }

    /// Return the original correction range for this wind metric.
    fn correction_range(&self) -> (f64, f64) {
        // map each admitted metric to the frozen domain policy
        match self.metric.as_str() {
            "windDirectionDegrees" => (-45.0, 45.0),
            "windGustMps" => (-12.0, 12.0),
            "windSpeedMps" => (-8.0, 8.0),
            _ => unreachable!("metric bands are admitted before policy lookup"),
        }
    }
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct CoefficientCell {
    band: MetricBand,
    level: i64,
    season: Option<String>,
    month: Option<i64>,
    daypart: Option<String>,
}

/// Validate the original closed, canonical wind candidate fitted material.
pub(crate) fn validate_wind_canary_candidate_material(candidate: &JsonValue) -> Option<()> {
    let enabled_nodes = rows(candidate, "enabledMetricBands")?;
    let coefficient_nodes = rows(candidate, "coefficients")?;
    let envelope_nodes = rows(candidate, "trainingEnvelopes")?;
    let mut enabled = Vec::with_capacity(enabled_nodes.len());

    // require a nonempty, unique pair list in exact ASCII canonical order
    for node in enabled_nodes {
        let band = MetricBand::read(node)?;
        if enabled.last().is_some_and(|previous| previous >= &band) {
            return None;
        }
        enabled.push(band);
    }
    // reject the empty enabled set retained by no valid Node candidate
    if enabled.is_empty() {
        return None;
    }

    let mut root_counts = vec![0_u8; enabled.len()];
    let mut previous_cell: Option<CoefficientCell> = None;

    // validate every fitted hierarchy cell and its original deterministic order
    for coefficient in coefficient_nodes {
        exact_keys(coefficient, COEFFICIENT_KEYS).then_some(())?;
        let band = MetricBand::read_fields(coefficient)?;
        let enabled_index = enabled
            .iter()
            .position(|candidate_band| candidate_band == &band)?;
        let coefficient_value = number_field(coefficient, "coefficient")?;
        let effective_event_count = number_field(coefficient, "effectiveEventCount")?;
        let (minimum, maximum) = band.correction_range();
        // retain finite nonnegative evidence and the metric-specific correction cap
        if effective_event_count < 0.0 || coefficient_value < minimum || coefficient_value > maximum
        {
            return None;
        }

        let level = integer_field(coefficient, "level")?;
        let daypart = nullable_text(coefficient, "daypart")?;
        let month = nullable_integer(coefficient, "month")?;
        let season = nullable_text(coefficient, "season")?;
        validate_coefficient_shape(
            level,
            daypart.as_deref(),
            month,
            season.as_deref(),
            effective_event_count,
        )?;
        // count the one mandatory root for each enabled pair
        if level == 1 {
            root_counts[enabled_index] = root_counts[enabled_index].checked_add(1)?;
        }

        let cell = CoefficientCell {
            band,
            level,
            season,
            month,
            daypart,
        };
        // reject both duplicate hierarchy cells and noncanonical fitted order
        if previous_cell
            .as_ref()
            .is_some_and(|previous| previous >= &cell)
        {
            return None;
        }
        previous_cell = Some(cell);
    }

    // require exactly one fitted root for every enabled pair
    if root_counts.iter().any(|count| *count != 1) {
        return None;
    }

    let expected_envelopes: Vec<&MetricBand> = enabled
        .iter()
        .filter(|band| band.metric != "windDirectionDegrees")
        .collect();
    // bind scalar envelopes one-for-one to sorted non-direction pairs
    if envelope_nodes.len() != expected_envelopes.len() {
        return None;
    }
    // validate exact envelope coverage, order, and finite scalar range
    for (envelope, expected) in envelope_nodes.iter().zip(expected_envelopes) {
        exact_keys(envelope, TRAINING_ENVELOPE_KEYS).then_some(())?;
        let actual = MetricBand::read_fields(envelope)?;
        let minimum = number_field(envelope, "minimum")?;
        let maximum = number_field(envelope, "maximum")?;
        // exclude direction, gaps, reorderings, and inverted scalar bounds
        if &actual != expected || actual.metric == "windDirectionDegrees" || minimum > maximum {
            return None;
        }
    }
    Some(())
}

/// Read one nullable string without replacing invalid UTF-16.
fn nullable_text(node: &JsonValue, key: &str) -> Option<Option<String>> {
    // preserve null separately from an admitted string
    match field(node, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) => Some(Some(String::from_utf16(units).ok()?)),
        _ => None,
    }
}

/// Read one nullable exactly represented integer.
fn nullable_integer(node: &JsonValue, key: &str) -> Option<Option<i64>> {
    // preserve null separately from an admitted integer
    match field(node, key)? {
        JsonValue::Null => Some(None),
        JsonValue::Number(_) => Some(Some(integer_field(node, key)?)),
        _ => None,
    }
}

/// Validate the root, seasonal, or monthly coefficient hierarchy shape.
fn validate_coefficient_shape(
    level: i64,
    daypart: Option<&str>,
    month: Option<i64>,
    season: Option<&str>,
    effective_event_count: f64,
) -> Option<()> {
    // admit only the root cell with the retained minimum evidence
    if level == 1 {
        return (daypart.is_none()
            && month.is_none()
            && season.is_none()
            && effective_event_count >= 200.0)
            .then_some(());
    }
    let valid_daypart = matches!(daypart, Some("afternoon" | "evening" | "morning" | "night"));
    // admit one seasonal refinement with no month
    if level == 2 {
        return (valid_daypart
            && month.is_none()
            && matches!(season, Some("autumn" | "spring" | "summer" | "winter"))
            && effective_event_count >= 100.0)
            .then_some(());
    }
    // admit one monthly refinement with no season
    if level == 3 {
        return (valid_daypart
            && season.is_none()
            && month.is_some_and(|value| (1..=12).contains(&value))
            && effective_event_count >= 50.0)
            .then_some(());
    }
    None
}
