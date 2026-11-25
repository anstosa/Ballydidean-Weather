//! receipt-backed wind performance cohort projection

use std::collections::BTreeMap;

use weather_contracts::{
    ForecastAdjustmentEvidenceClass, ForecastAdjustmentMetric, ForecastAdjustmentPerformancePair,
    JsonValue, Nullable,
};
use weather_domain::forecast_lead_band_for;

use crate::maintenance_canonical_json::field;
use crate::rain_performance_edge_rows::JoinedRainPerformanceEdgeRow;
use crate::temperature_performance_target::{
    TemperaturePerformanceTargetValues, derive_wind_performance_target,
};
use crate::wind_performance_common::{
    WindPerformanceBand, WindPerformanceError, band_json, band_key, identity, lead_band_wire,
    metric_wire, object, require_object, required_number, required_text, wire,
};

/// receipt-backed pairs and captured fail-raw reason counts
#[derive(Debug, Clone, PartialEq)]
pub struct IssuedWindPerformancePairs {
    pub pairs: BTreeMap<String, Vec<ForecastAdjustmentPerformancePair>>,
    pub disabled_count: u64,
    pub not_applicable_count: u64,
}

/// collect the disjoint receipt-backed cohort from the selected bundle
pub fn collect_issued_wind_pairs(
    forecast_rows: &[JsonValue],
    adjustment_rows: &[JsonValue],
    edges: &[JoinedRainPerformanceEdgeRow],
    bundle_sha256: &str,
    mask: &[WindPerformanceBand],
    values: &mut impl TemperaturePerformanceTargetValues,
) -> Result<IssuedWindPerformancePairs, WindPerformanceError> {
    let mut pairs = BTreeMap::new();
    // initialize every enabled pair even when its cohort remains empty
    for definition in mask {
        pairs.insert(band_key(definition), Vec::new());
    }

    let enabled_speed = mask
        .iter()
        .any(|definition| definition.metric == ForecastAdjustmentMetric::WindSpeedMps);
    let enabled_gust = mask
        .iter()
        .any(|definition| definition.metric == ForecastAdjustmentMetric::WindGustMps);
    let mut disabled_count = 0_u64;
    let mut not_applicable_count = 0_u64;

    // preserve verified edge package order
    for edge in edges {
        let bundles = required_object_field(&edge.bundle_identities, "wind", "edge wind bundle")?;

        // retain only rows served by the exact active bundle under review
        if !string_field_equals(bundles, "activeBundle", bundle_sha256) {
            continue;
        }

        let record = required_object_field(&edge.row, "record", "edge wind record")?;
        let adjustment =
            required_object_field(&edge.row, "windAdjustment", "edge wind adjustment")?;
        let raw = required_object_field(&edge.row, "raw", "edge wind raw metrics")?;
        let adjusted =
            required_object_field(adjustment, "adjustedMetrics", "edge adjusted wind metrics")?;
        let valid_at = required_text(record, "validAt", "edge wind validAt")?;
        let applied_metrics = field(adjustment, "appliedMetrics");
        let state = required_text(adjustment, "state", "edge wind state")?;
        let valid_at_epoch = values.utc_epoch_milliseconds(&valid_at);

        // reject malformed serving details before descriptive scoring
        if valid_at_epoch.is_none() || !matches!(applied_metrics, Some(JsonValue::Array(_))) {
            return Err(WindPerformanceError::new(
                "edge wind adjustment identity is invalid",
            ));
        }

        // reject unknown states instead of treating them as raw fallbacks
        if state != "active" && state != "disabled" && state != "not_applicable" {
            return Err(WindPerformanceError::new(
                "edge wind adjustment state is invalid",
            ));
        }

        // count fail-raw decisions without inventing a lead band
        if state != "active" {
            count_non_active_metric(
                raw,
                "windSpeedMps",
                enabled_speed,
                &state,
                &mut disabled_count,
                &mut not_applicable_count,
            )?;
            count_non_active_metric(
                raw,
                "windGustMps",
                enabled_gust,
                &state,
                &mut disabled_count,
                &mut not_applicable_count,
            )?;
            continue;
        }

        let product_run_at = required_text(record, "productRunAt", "edge wind product run")?;
        let product_run_epoch = values.utc_epoch_milliseconds(&product_run_at);
        let continuous_horizon_hours = product_run_epoch.map(|product_run_epoch| {
            (valid_at_epoch.expect("validated valid instant") - product_run_epoch) as f64
                / 3_600_000.0
        });

        // require a finite positive runtime lead only for active decisions
        if !continuous_horizon_hours.is_some_and(|hours| hours.is_finite() && hours > 0.0) {
            return Err(WindPerformanceError::new(
                "edge wind adjustment identity is invalid",
            ));
        }

        let horizon_hours = continuous_horizon_hours
            .expect("validated active horizon")
            .ceil();
        let lead_band = required_text(adjustment, "leadBand", "edge wind lead band")?;
        let expected_lead_band = forecast_lead_band_for(horizon_hours)
            .map_err(|error| WindPerformanceError::new(error.to_string()))?;

        // bind the captured band to the runtime ceiling policy
        if lead_band != lead_band_wire(expected_lead_band) {
            return Err(WindPerformanceError::new("edge wind lead band is invalid"));
        }

        // score enabled definitions independently in their original order
        for definition in mask {
            let Some(metric_field) = target_metric_field(definition.metric) else {
                continue;
            };

            // retain only the captured row's exact lead band
            if lead_band_wire(definition.lead_band) != lead_band {
                continue;
            }

            let metric = metric_wire(definition.metric);
            let raw_value = field(raw, metric);

            // preserve a missing captured source metric as a non-event
            if raw_value.is_none() || matches!(raw_value, Some(JsonValue::Null)) {
                continue;
            }

            let target = derive_wind_performance_target(
                forecast_rows,
                &valid_at,
                metric_field,
                adjustment_rows,
                values,
            )
            .map_err(|error| WindPerformanceError::new(error.to_string()))?;
            let was_applied = array_includes_text(applied_metrics, metric);
            // select the captured adjustment or its raw fallback
            let adjusted_prediction = if was_applied {
                required_number(adjusted, metric, "edge adjusted wind value")?
            } else {
                required_number(raw, metric, "edge raw wind value")?
            };
            let local_date = values
                .los_angeles_local_date(valid_at_epoch.expect("validated valid instant"))
                .ok_or_else(|| {
                    WindPerformanceError::new("ICU did not return complete local calendar features")
                })?;
            let raw_prediction = required_number(raw, metric, "edge raw wind value")?;
            let row_identity = issued_row_identity(edge, bundles, record, definition, values)?;
            let pair_key = band_key(definition);
            // retain the original explicit missing target identity
            let (target_key, target_value) = match target {
                Some((identity, value)) => (identity, Nullable::Value(value)),
                None => (format!("missing:{valid_at}:{metric}"), Nullable::Null),
            };
            let pair = ForecastAdjustmentPerformancePair {
                adjustedPrediction: adjusted_prediction,
                evidenceClass: ForecastAdjustmentEvidenceClass::AsIssued,
                fallback: !was_applied,
                firstEdgeCommittedAt: Nullable::Value(edge.first_edge_committed_at.clone()),
                horizonHours: horizon_hours,
                key: format!("{pair_key}:{product_run_at}:{valid_at}"),
                localDate: local_date,
                provenanceComplete: boolean_field_equals(&edge.row, "provenanceComplete", true),
                rawPrediction: raw_prediction,
                rowIdentity: row_identity,
                sourceReceiptAt: Nullable::Value(edge.source_receipt_at.clone()),
                target: target_value,
                targetKey: target_key,
                validAt: valid_at.clone(),
                vintageKey: product_run_at.clone(),
            };
            pairs
                .get_mut(&pair_key)
                .expect("enabled wind pair was initialized")
                .push(pair);
        }
    }

    Ok(IssuedWindPerformancePairs {
        pairs,
        disabled_count,
        not_applicable_count,
    })
}

/// require one named nested object with the predecessor diagnostic
fn required_object_field<'a>(
    value: &'a JsonValue,
    key: &str,
    description: &str,
) -> Result<&'a JsonValue, WindPerformanceError> {
    let nested = field(value, key)
        .ok_or_else(|| WindPerformanceError::new(format!("{description} must be an object")))?;
    require_object(nested, description)
}

/// count one captured enabled metric for a nonactive serving decision
fn count_non_active_metric(
    raw: &JsonValue,
    metric: &str,
    enabled: bool,
    state: &str,
    disabled_count: &mut u64,
    not_applicable_count: &mut u64,
) -> Result<(), WindPerformanceError> {
    let raw_value = field(raw, metric);
    // ignore disabled definitions and absent captured metrics
    if !enabled || raw_value.is_none() || matches!(raw_value, Some(JsonValue::Null)) {
        return Ok(());
    }

    required_number(raw, metric, "edge raw wind value")?;
    // count the exact nonactive reason after numeric admission
    if state == "disabled" {
        *disabled_count += 1;
    } else {
        *not_applicable_count += 1;
    }
    Ok(())
}

/// build one issued row identity with javascript undefined fields omitted
fn issued_row_identity(
    edge: &JoinedRainPerformanceEdgeRow,
    bundles: &JsonValue,
    record: &JsonValue,
    definition: &WindPerformanceBand,
    values: &mut impl TemperaturePerformanceTargetValues,
) -> Result<String, WindPerformanceError> {
    let active_bundle = field(bundles, "activeBundle")
        .cloned()
        .expect("selected edge has an active bundle");
    let mut fields = vec![
        ("activeBundle", active_bundle),
        ("definition", band_json(definition)),
    ];
    // omit a missing record identifier like JSON.stringify undefined
    if let Some(record_id) = field(record, "id") {
        fields.push(("recordId", record_id.clone()));
    }
    // omit a missing record revision like JSON.stringify undefined
    if let Some(record_revision) = field(record, "revisionCount") {
        fields.push(("recordRevision", record_revision.clone()));
    }
    fields.push(("settingsSha256", wire(&edge.settings_sha256)));
    identity(&object(fields), values)
}

/// return the forecast export field for one supported wind metric
fn target_metric_field(metric: ForecastAdjustmentMetric) -> Option<&'static str> {
    // exclude direction and nonwind definitions from scoring
    match metric {
        ForecastAdjustmentMetric::WindSpeedMps => Some("wind_speed_mps"),
        ForecastAdjustmentMetric::WindGustMps => Some("wind_gust_mps"),
        ForecastAdjustmentMetric::RelativeHumidityPercent
        | ForecastAdjustmentMetric::TemperatureC
        | ForecastAdjustmentMetric::WindDirectionDegrees => None,
    }
}

/// compare one object string member without normalizing utf-sixteen
fn string_field_equals(value: &JsonValue, key: &str, expected: &str) -> bool {
    matches!(field(value, key), Some(JsonValue::String(actual))
        if actual.iter().copied().eq(expected.encode_utf16()))
}

/// compare one object boolean member exactly
fn boolean_field_equals(value: &JsonValue, key: &str, expected: bool) -> bool {
    matches!(field(value, key), Some(JsonValue::Boolean(actual)) if *actual == expected)
}

/// reproduce array includes for one metric string
fn array_includes_text(array: Option<&JsonValue>, expected: &str) -> bool {
    // reject absent or scalar applied metric payloads
    let Some(JsonValue::Array(entries)) = array else {
        return false;
    };
    entries.iter().any(|entry| {
        matches!(entry, JsonValue::String(actual)
            if actual.iter().copied().eq(expected.encode_utf16()))
    })
}

#[cfg(test)]
#[path = "wind_performance_issued_test.rs"]
mod tests;
