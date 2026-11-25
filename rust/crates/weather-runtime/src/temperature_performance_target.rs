//! original regional temperature target and revision-evidence identity

use std::cmp::Ordering;
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{
    ForecastObservationStationKey, JsonValue, retained_forecast_observation::STATIONS,
};

use crate::forecast_training_package_row::ForecastTrainingPackageRowTime;
use crate::maintenance_canonical_json::{MaintenanceCanonicalJson, field, text};

const HOUR_MILLISECONDS: i64 = 3_600_000;
const MINIMUM_NETWORK_STATIONS: usize = 3;
const NEAREST_ELIGIBLE_STATION_COUNT: u64 = 3;
const ECOWITT_STATION_KEY: &str = "ballydidean-ecowitt";

/// supplied canonical, clock, and javascript locale effects
pub trait TemperaturePerformanceTargetValues:
    MaintenanceCanonicalJson + ForecastTrainingPackageRowTime
{
    /// compare two strings with the original `localeCompare` ordering
    fn compare_js_locale(&self, left: &str, right: &str) -> Ordering;
}

/// one original temperature target join result
#[derive(Debug, Clone, PartialEq)]
pub struct TemperaturePerformanceTarget {
    pub max_receipt_at: Option<String>,
    pub target: Option<f64>,
    pub target_identity: Option<String>,
}

/// rejected target input or unavailable canonical effect
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemperaturePerformanceTargetError {
    Canonical,
    DuplicatedStation,
    InvalidRow,
    InvalidStationValue,
    UnknownStation,
}

impl Display for TemperaturePerformanceTargetError {
    /// describe one bounded target derivation failure
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve stable failure categories for the projection boundary
        formatter.write_str(match self {
            Self::Canonical => "temperature target canonical identity is unavailable",
            Self::DuplicatedStation => "physical station target is duplicated",
            Self::InvalidRow => "temperature target row is invalid",
            Self::InvalidStationValue => "spatial station values are invalid or duplicated",
            Self::UnknownStation => "physical station target identity is unknown",
        })
    }
}

impl Error for TemperaturePerformanceTargetError {}

#[derive(Debug, Clone)]
struct SelectedTargetRow<'a> {
    row: &'a JsonValue,
    station_key: String,
    value: f64,
}

#[derive(Debug, Clone)]
struct WeightedTargetRow<'a> {
    row: &'a JsonValue,
    station_key: String,
    nearest_rank: u64,
    weight: f64,
    value: f64,
}

/// derive one regional temperature target and immutable target identity
pub fn derive_temperature_performance_target(
    forecast_rows: &[JsonValue],
    valid_at: &str,
    adjustment_rows: &[JsonValue],
    values: &mut impl TemperaturePerformanceTargetValues,
) -> Result<TemperaturePerformanceTarget, TemperaturePerformanceTargetError> {
    derive_regional_target(
        forecast_rows,
        valid_at,
        "temperature_c",
        adjustment_rows,
        values,
        true,
    )
}

/// Derive the wind value and revision identity without temperature receipt admission.
pub(crate) fn derive_wind_performance_target(
    forecast_rows: &[JsonValue],
    valid_at: &str,
    metric_field: &str,
    adjustment_rows: &[JsonValue],
    values: &mut impl TemperaturePerformanceTargetValues,
) -> Result<Option<(String, f64)>, TemperaturePerformanceTargetError> {
    let target = derive_regional_target(
        forecast_rows,
        valid_at,
        metric_field,
        adjustment_rows,
        values,
        false,
    )?;
    Ok(target.target_identity.zip(target.target))
}

/// Bind the existing regional scalar algorithm and shared revision preimage.
fn derive_regional_target(
    forecast_rows: &[JsonValue],
    valid_at: &str,
    metric_field: &str,
    adjustment_rows: &[JsonValue],
    values: &mut impl TemperaturePerformanceTargetValues,
    bind_maximum_receipt: bool,
) -> Result<TemperaturePerformanceTarget, TemperaturePerformanceTargetError> {
    let selected = select_target_rows(forecast_rows, valid_at, metric_field)?;
    let weighted = resolve_station_values(selected)?;
    let Some(target) = scalar_network_actual(&weighted, values)? else {
        return Ok(missing_target());
    };

    let revision_evidence = target_revision_evidence(adjustment_rows, &weighted, valid_at, values)?;
    // wind preserves revision evidence without reading a separate temperature receipt field
    let max_receipt_at = if bind_maximum_receipt {
        maximum_receipt(&weighted, &revision_evidence)?
    } else {
        None
    };
    let selected_target = selected_target_identity(&weighted, values);
    let preimage = object(vec![
        ("selectedTarget", JsonValue::Array(selected_target)),
        ("revisionEvidence", JsonValue::Array(revision_evidence)),
    ]);
    let encoded = values
        .encode_canonical_json(&preimage)
        .ok_or(TemperaturePerformanceTargetError::Canonical)?;
    let target_identity = values
        .sha256(&encoded)
        .map(|digest| digest.as_str().to_owned())
        .ok_or(TemperaturePerformanceTargetError::Canonical)?;

    Ok(TemperaturePerformanceTarget {
        max_receipt_at,
        target: Some(target),
        target_identity: Some(target_identity),
    })
}

/// select and map the original station-hour rows for the requested scalar metric
fn select_target_rows<'a>(
    forecast_rows: &'a [JsonValue],
    valid_at: &str,
    metric_field: &str,
) -> Result<Vec<SelectedTargetRow<'a>>, TemperaturePerformanceTargetError> {
    let mut selected = Vec::new();
    // retain source order until the original target builder sorts values
    for row in forecast_rows {
        // reproduce strict property equality and explicit null exclusion
        if !string_field_equals(row, "record_kind", "station_hour")
            || !string_field_equals(row, "valid_at", valid_at)
            || string_field_equals(row, "physical_station_key", ECOWITT_STATION_KEY)
            || matches!(field(row, metric_field), Some(JsonValue::Null))
        {
            continue;
        }
        let station_key = text(row, "physical_station_key")
            .ok_or(TemperaturePerformanceTargetError::InvalidRow)?;
        let Some(JsonValue::Number(value)) = field(row, metric_field) else {
            return Err(TemperaturePerformanceTargetError::InvalidStationValue);
        };
        // preserve the `finite` helper before station identity validation
        if !value.is_finite() {
            return Err(TemperaturePerformanceTargetError::InvalidStationValue);
        }
        selected.push(SelectedTargetRow {
            row,
            station_key,
            value: *value,
        });
    }
    Ok(selected)
}

/// resolve frozen ranks and weights after every selected value is mapped
fn resolve_station_values<'a>(
    selected: Vec<SelectedTargetRow<'a>>,
) -> Result<Vec<WeightedTargetRow<'a>>, TemperaturePerformanceTargetError> {
    let mut identities = HashSet::with_capacity(selected.len());
    let mut weighted = Vec::with_capacity(selected.len());
    // preserve duplicate and unknown checks in selected source order
    for selected_row in selected {
        // reject a repeated physical identity before roster lookup
        if !identities.insert(selected_row.station_key.clone()) {
            return Err(TemperaturePerformanceTargetError::DuplicatedStation);
        }
        let station_key = ForecastObservationStationKey::WIRE_VALUES
            .iter()
            .find_map(|(wire, key)| (*wire == selected_row.station_key.as_str()).then_some(*key))
            .ok_or(TemperaturePerformanceTargetError::UnknownStation)?;
        let station = STATIONS
            .iter()
            .find(|station| station.key == station_key)
            .ok_or(TemperaturePerformanceTargetError::UnknownStation)?;
        weighted.push(WeightedTargetRow {
            row: selected_row.row,
            station_key: selected_row.station_key,
            nearest_rank: station.nearest_rank,
            weight: station.unnormalized_spatial_weight,
            value: selected_row.value,
        });
    }
    Ok(weighted)
}

/// compute the exact normalized strict-half spatial weighted median
fn scalar_network_actual(
    stations: &[WeightedTargetRow<'_>],
    values: &impl TemperaturePerformanceTargetValues,
) -> Result<Option<f64>, TemperaturePerformanceTargetError> {
    // return missing before numeric validation when physical coverage fails
    if stations.len() < MINIMUM_NETWORK_STATIONS
        || !stations
            .iter()
            .any(|station| station.nearest_rank <= NEAREST_ELIGIBLE_STATION_COUNT)
    {
        return Ok(None);
    }
    let mut normalized = stations.to_vec();
    normalized
        .sort_by(|left, right| values.compare_js_locale(&left.station_key, &right.station_key));
    let mut total_weight = 0.0;
    // sum weights in javascript locale station order
    for station in &normalized {
        // retain the original finite positive station validation
        if station.nearest_rank == 0
            || !station.value.is_finite()
            || !station.weight.is_finite()
            || station.weight <= 0.0
        {
            return Err(TemperaturePerformanceTargetError::InvalidStationValue);
        }
        total_weight += station.weight;
    }
    // reject nonfinite or zero aggregate mass
    if !total_weight.is_finite() || total_weight <= 0.0 {
        return Err(TemperaturePerformanceTargetError::InvalidStationValue);
    }
    normalized.sort_by(|left, right| {
        left.value
            .partial_cmp(&right.value)
            .expect("validated finite temperature target")
            .then_with(|| values.compare_js_locale(&left.station_key, &right.station_key))
    });
    let mut normalized_total = 0.0;
    let mut distinct = Vec::<(f64, f64)>::with_capacity(normalized.len());
    // combine exact equal values after normalization
    for station in normalized {
        let normalized_weight = station.weight / total_weight;
        normalized_total += normalized_weight;
        // add exact value ties to the preceding cell
        if let Some(last) = distinct.last_mut()
            && last.0 == station.value
        {
            last.1 += normalized_weight;
        } else {
            distinct.push((station.value, normalized_weight));
        }
    }
    // retain the weighted-median helper's second total in value order
    if !normalized_total.is_finite() || normalized_total <= 0.0 {
        return Err(TemperaturePerformanceTargetError::InvalidStationValue);
    }
    let mut cumulative_weight = 0.0;
    let half_weight = normalized_total / 2.0;
    // select the first strict half crossing or exact midpoint tie
    for (index, (value, weight)) in distinct.iter().enumerate() {
        cumulative_weight += weight;
        // preserve the first strict half crossing
        if cumulative_weight > half_weight {
            return Ok(Some(*value));
        }
        // average the next greater value at an exact half tie
        if cumulative_weight == half_weight {
            return Ok(Some(
                distinct
                    .get(index + 1)
                    .map_or(*value, |next| (*value + next.0) / 2.0),
            ));
        }
    }
    Ok(distinct.last().map(|last| last.0))
}

/// bind matching preceding-hour revision payloads and canonical order
fn target_revision_evidence(
    adjustment_rows: &[JsonValue],
    target_rows: &[WeightedTargetRow<'_>],
    valid_at: &str,
    values: &mut impl TemperaturePerformanceTargetValues,
) -> Result<Vec<JsonValue>, TemperaturePerformanceTargetError> {
    let interval = values
        .utc_epoch_milliseconds(valid_at)
        .and_then(|end| end.checked_sub(HOUR_MILLISECONDS).map(|start| (start, end)));
    let mut matched = Vec::new();
    // inspect only the predecessor's diagnostic record kind
    for row in adjustment_rows {
        // skip unrelated adjustment package rows
        if !string_field_equals(row, "record_kind", "target_revision_diagnostic") {
            continue;
        }
        let payload = field(row, "payload")
            .filter(|payload| matches!(payload, JsonValue::Object(_)))
            .ok_or(TemperaturePerformanceTargetError::InvalidRow)?;
        let hour_text =
            text(payload, "validAt").ok_or(TemperaturePerformanceTargetError::InvalidRow)?;
        let hour = values.utc_epoch_milliseconds(&hour_text);
        let in_interval = matches!(
            (interval, hour),
            (Some((start, end)), Some(hour)) if hour >= start && hour <= end
        );
        // retain only evidence bound to one selected physical source lineage
        if in_interval
            && target_rows
                .iter()
                .any(|target| revision_matches_target(payload, target.row))
        {
            let canonical = values
                .encode_canonical_json(payload)
                .ok_or(TemperaturePerformanceTargetError::Canonical)?;
            let canonical = String::from_utf8(canonical)
                .map_err(|_| TemperaturePerformanceTargetError::Canonical)?;
            matched.push((payload.clone(), canonical));
        }
    }
    matched.sort_by(|left, right| values.compare_js_locale(&left.1, &right.1));
    Ok(matched.into_iter().map(|(payload, _)| payload).collect())
}

/// compare one diagnostic payload to one selected station source lineage
fn revision_matches_target(payload: &JsonValue, target: &JsonValue) -> bool {
    let station_matches = field(target, "physical_station_key")
        .zip(field(payload, "physicalStationKey"))
        .is_some_and(|(left, right)| left == right);
    let source_matches = array_includes(field(target, "source_keys"), field(payload, "sourceKey"));
    let fingerprint_matches = array_includes(
        field(target, "source_config_fingerprints"),
        field(payload, "sourceConfigFingerprint"),
    );
    station_matches && source_matches && fingerprint_matches
}

/// find the latest source or diagnostic receipt with default javascript sorting
fn maximum_receipt(
    target_rows: &[WeightedTargetRow<'_>],
    revision_evidence: &[JsonValue],
) -> Result<Option<String>, TemperaturePerformanceTargetError> {
    let mut receipts = Vec::<Vec<u16>>::new();
    // include only string station receipts like the original `flatMap`
    for target in target_rows {
        // retain a present string without coercion
        if let Some(JsonValue::String(received_at)) = field(target.row, "received_at") {
            receipts.push(received_at.clone());
        }
    }
    // require every matched revision's maximum source receipt text
    for evidence in revision_evidence {
        let Some(JsonValue::String(received_at)) = field(evidence, "maxSourceReceiptAt") else {
            return Err(TemperaturePerformanceTargetError::InvalidRow);
        };
        receipts.push(received_at.clone());
    }
    receipts.sort();
    receipts
        .pop()
        .map(|receipt| {
            String::from_utf16(&receipt).map_err(|_| TemperaturePerformanceTargetError::InvalidRow)
        })
        .transpose()
}

/// build canonical selected-target rows in locale station order
fn selected_target_identity(
    target_rows: &[WeightedTargetRow<'_>],
    values: &impl TemperaturePerformanceTargetValues,
) -> Vec<JsonValue> {
    let mut identity = target_rows
        .iter()
        .map(|target| {
            let mut fields = Vec::with_capacity(5);
            // omit absent properties like `JSON.stringify` omits `undefined`
            for (output_key, source_key) in [
                ("contentHashes", "content_hashes"),
                ("physicalStationKey", "physical_station_key"),
                ("receivedAt", "received_at"),
                ("sourceKeys", "source_keys"),
                ("validAt", "valid_at"),
            ] {
                // preserve null and every present json value without coercion
                if let Some(value) = field(target.row, source_key) {
                    fields.push((output_key.encode_utf16().collect(), value.clone()));
                }
            }
            (target.station_key.clone(), JsonValue::Object(fields))
        })
        .collect::<Vec<_>>();
    identity.sort_by(|left, right| values.compare_js_locale(&left.0, &right.0));
    identity.into_iter().map(|(_, row)| row).collect()
}

/// report one explicit coverage gap
fn missing_target() -> TemperaturePerformanceTarget {
    TemperaturePerformanceTarget {
        max_receipt_at: None,
        target: None,
        target_identity: None,
    }
}

/// compare one object string field without allocating a rust string
fn string_field_equals(value: &JsonValue, key: &str, expected: &str) -> bool {
    matches!(field(value, key), Some(JsonValue::String(actual))
        if actual.iter().copied().eq(expected.encode_utf16()))
}

/// reproduce `Array.includes` for one present json payload member
fn array_includes(array: Option<&JsonValue>, needle: Option<&JsonValue>) -> bool {
    // json arrays cannot retain the predecessor's `undefined` needle
    let (Some(JsonValue::Array(values)), Some(needle)) = (array, needle) else {
        return false;
    };
    values.iter().any(|value| value == needle)
}

/// build one json object from utf-eight field names
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

#[cfg(test)]
#[path = "temperature_performance_target_test.rs"]
mod tests;
