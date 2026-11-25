//! Retrospective wind pairs replayed from original live-v4 forecast rows.

use std::collections::BTreeMap;

use weather_contracts::{
    CanonicalWeatherMetrics, ForecastAdjustmentEvidenceClass, ForecastAdjustmentMetric,
    ForecastAdjustmentPerformancePair, ForecastAdjustmentRawForecastProvenance,
    ForecastAdjustmentRawForecastProvenanceCohortLiteral,
    ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral, ForecastLeadBandKey, JsonValue,
    Nullable,
};

use crate::maintenance_canonical_json::field;
use crate::temperature_performance_target::{
    TemperaturePerformanceTargetValues, derive_wind_performance_target,
};
use crate::wind_performance_common::{
    WindPerformanceBand, WindPerformanceError, band_json, band_key, identity, metric_wire, object,
    required_number, required_text, wire,
};

/// one active native replay result for the original raw forecast row
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindPerformanceReplayDecision {
    pub lead_band: ForecastLeadBandKey,
    pub adjusted_wind_speed_mps: Option<f64>,
    pub adjusted_wind_gust_mps: Option<f64>,
}

/// approved effects for replaying an admitted wind row through the native kernel
pub trait WindPerformanceCounterfactualValues: TemperaturePerformanceTargetValues {
    /// replay one fully decoded source row through the verified active bundle
    fn replay_admitted_wind_forecast(
        &mut self,
        generated_at: &str,
        metrics: &CanonicalWeatherMetrics,
        provenance: &ForecastAdjustmentRawForecastProvenance,
    ) -> Result<Option<WindPerformanceReplayDecision>, WindPerformanceError>;
}

/// build the retrospective raw-row populations for every active wind mask band
pub fn collect_counterfactual_wind_pairs(
    forecast_rows: &[JsonValue],
    adjustment_rows: &[JsonValue],
    mask: &[WindPerformanceBand],
    generated_at: &str,
    values: &mut impl WindPerformanceCounterfactualValues,
) -> Result<BTreeMap<String, Vec<ForecastAdjustmentPerformancePair>>, WindPerformanceError> {
    let mut pairs_by_band = BTreeMap::new();
    // initialize exactly the active mask keys before observing any source rows
    for definition in mask {
        pairs_by_band.insert(band_key(definition), Vec::new());
    }

    // retain only original live-v4 retrieval snapshots in package source order
    for row in forecast_rows {
        if !string_field_equals(row, "record_kind", "legacy_v4_retrieval_snapshot") {
            continue;
        }
        let valid_at = required_text(row, "valid_at", "wind validAt")?;
        let target_lead_hours = required_number(row, "target_lead_hours", "wind target lead")?;
        let metrics = wind_metrics(row)?;
        let provenance = decoded_raw_forecast_provenance(row)?;
        // keep unsupported finite leads as the original non-active replay gap
        let decision = if weather_domain::forecast_lead_band_for(target_lead_hours).is_ok() {
            let provenance = provenance.into_generated(target_lead_hours as u64);
            values.replay_admitted_wind_forecast(generated_at, &metrics, &provenance)?
        } else {
            None
        };
        // score speed and gust independently against the row's active lead band
        for definition in mask {
            // exclude direction if a future runtime widens the supplied mask
            if !matches!(
                definition.metric,
                ForecastAdjustmentMetric::WindSpeedMps | ForecastAdjustmentMetric::WindGustMps
            ) {
                continue;
            }
            // retain only active native decisions
            let Some(active) = decision else {
                continue;
            };
            // retain only the row's exact enabled lead band
            if definition.lead_band != active.lead_band {
                continue;
            }
            // select the original snake-case source field and adjusted value
            let (metric_field, adjusted) = match definition.metric {
                ForecastAdjustmentMetric::WindSpeedMps => {
                    ("wind_speed_mps", active.adjusted_wind_speed_mps)
                }
                ForecastAdjustmentMetric::WindGustMps => {
                    ("wind_gust_mps", active.adjusted_wind_gust_mps)
                }
                _ => unreachable!("non-wind definitions were excluded"),
            };
            // preserve explicit source nulls as non-events without admitting absent fields
            if matches!(field(row, metric_field), Some(JsonValue::Null)) {
                continue;
            }
            let raw = required_number(row, metric_field, "wind raw value")?;
            let target = derive_wind_performance_target(
                forecast_rows,
                &valid_at,
                metric_field,
                adjustment_rows,
                values,
            )
            .map_err(|error| WindPerformanceError::new(error.to_string()))?;
            let pair_key = band_key(definition);
            let pairs = pairs_by_band
                .get_mut(&pair_key)
                .ok_or_else(|| WindPerformanceError::new("wind pair is outside the active mask"))?;
            let reference_at = required_text(row, "reference_at", "wind referenceAt")?;
            let local_date = local_date(&valid_at, values)?;
            let row_identity =
                counterfactual_identity(row, definition, &reference_at, &valid_at, values)?;
            let (target_key, target_value) = target.map_or_else(
                || {
                    (
                        format!("missing:{valid_at}:{}", metric_wire(definition.metric)),
                        Nullable::Null,
                    )
                },
                |(identity, value)| (identity, Nullable::Value(value)),
            );
            pairs.push(ForecastAdjustmentPerformancePair {
                adjustedPrediction: adjusted.unwrap_or(raw),
                evidenceClass: ForecastAdjustmentEvidenceClass::RetrospectiveCounterfactual,
                fallback: adjusted.is_none(),
                firstEdgeCommittedAt: Nullable::Null,
                horizonHours: target_lead_hours,
                key: format!("{pair_key}:{reference_at}:{valid_at}"),
                localDate: local_date.clone(),
                provenanceComplete: true,
                rawPrediction: raw,
                rowIdentity: row_identity,
                sourceReceiptAt: Nullable::Null,
                target: target_value,
                targetKey: target_key,
                validAt: valid_at.clone(),
                vintageKey: reference_at,
            });
        }
    }
    Ok(pairs_by_band)
}

/// decode the exact nullable weather metrics passed to the native replay kernel
fn wind_metrics(row: &JsonValue) -> Result<CanonicalWeatherMetrics, WindPerformanceError> {
    Ok(CanonicalWeatherMetrics {
        apparentTemperatureC: Nullable::Null,
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Null,
        pm25MicrogramsPerCubicMeter: Nullable::Null,
        precipitationMm: Nullable::Null,
        precipitationRateMmPerHour: Nullable::Null,
        pressureHpa: Nullable::Null,
        relativeHumidityPercent: nullable_number(row, "relative_humidity_percent", "raw humidity")?,
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: nullable_number(row, "temperature_c", "raw temperature")?,
        uvIndex: Nullable::Null,
        waterLevelM: Nullable::Null,
        windDirectionDegrees: nullable_number(row, "wind_direction_degrees", "raw direction")?,
        windGustMps: nullable_number(row, "wind_gust_mps", "raw gust")?,
        windSpeedMps: nullable_number(row, "wind_speed_mps", "raw speed")?,
        wetBulbGlobeTemperatureC: Nullable::Null,
    })
}

/// decode the exact retrieval-snapshot provenance sent to the admitted kernel
fn decoded_raw_forecast_provenance(
    row: &JsonValue,
) -> Result<DecodedRawForecastProvenance, WindPerformanceError> {
    Ok(DecodedRawForecastProvenance {
        adapter_version: required_first_text(row, "adapter_contracts", "wind adapter version")?,
        contract_epoch: required_text(row, "contract_epoch", "wind contract epoch")?,
        dataset: required_text(row, "dataset", "wind dataset")?,
        reference_at: required_text(row, "reference_at", "wind referenceAt")?,
        source_config_fingerprint: required_first_text(
            row,
            "source_config_fingerprints",
            "wind source fingerprint",
        )?,
        source_key: required_first_text(row, "source_keys", "wind source key")?,
        upstream_model: required_text(row, "upstream_model", "wind upstream model")?,
        valid_at: required_text(row, "valid_at", "wind validAt")?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DecodedRawForecastProvenance {
    adapter_version: String,
    contract_epoch: String,
    dataset: String,
    reference_at: String,
    source_config_fingerprint: String,
    source_key: String,
    upstream_model: String,
    valid_at: String,
}

impl DecodedRawForecastProvenance {
    /// bind a lead already admitted by the shared generated domain policy
    fn into_generated(self, target_lead_hours: u64) -> ForecastAdjustmentRawForecastProvenance {
        ForecastAdjustmentRawForecastProvenance {
            adapterVersion: self.adapter_version,
            cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
            contractEpoch: self.contract_epoch,
            dataset: self.dataset,
            referenceAt: self.reference_at,
            referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
            sourceConfigFingerprint: self.source_config_fingerprint,
            sourceKey: self.source_key,
            targetLeadHours: target_lead_hours,
            upstreamModel: self.upstream_model,
            validAt: self.valid_at,
        }
    }
}

/// retain explicit null while rejecting absent, nonnumeric, or nonfinite values
fn nullable_number(
    row: &JsonValue,
    key: &str,
    description: &str,
) -> Result<Nullable<f64>, WindPerformanceError> {
    // distinguish the predecessor's explicit null from every malformed optional field
    if matches!(field(row, key), Some(JsonValue::Null)) {
        return Ok(Nullable::Null);
    }
    required_number(row, key, description).map(Nullable::Value)
}

/// decode the first source-lineage array member without accepting later fallbacks
fn required_first_text(
    row: &JsonValue,
    key: &str,
    description: &str,
) -> Result<String, WindPerformanceError> {
    let Some(JsonValue::Array(values)) = field(row, key) else {
        return Err(WindPerformanceError::new(format!(
            "{description} must be a nonempty string"
        )));
    };
    // preserve the original `[0]` selection rather than searching later entries
    match values.first() {
        Some(JsonValue::String(value)) => String::from_utf16(value)
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                WindPerformanceError::new(format!("{description} must be a nonempty string"))
            }),
        _ => Err(WindPerformanceError::new(format!(
            "{description} must be a nonempty string"
        ))),
    }
}

/// resolve the predecessor's Los Angeles calendar date for one valid instant
fn local_date(
    valid_at: &str,
    values: &impl TemperaturePerformanceTargetValues,
) -> Result<String, WindPerformanceError> {
    let epoch_ms = values
        .utc_epoch_milliseconds(valid_at)
        .ok_or_else(|| WindPerformanceError::new("wind validAt is invalid"))?;
    values
        .los_angeles_local_date(epoch_ms)
        .ok_or_else(|| WindPerformanceError::new("wind local date is unavailable"))
}

/// hash the original raw row's immutable counterfactual identity document
fn counterfactual_identity(
    row: &JsonValue,
    definition: &WindPerformanceBand,
    reference_at: &str,
    valid_at: &str,
    values: &mut impl TemperaturePerformanceTargetValues,
) -> Result<String, WindPerformanceError> {
    let mut fields = Vec::with_capacity(4);
    // match JSON.stringify omission only when the verified property is absent
    if let Some(content_hashes) = field(row, "content_hashes") {
        fields.push(("contentHashes", content_hashes.clone()));
    }
    fields.extend([
        ("definition", band_json(definition)),
        ("referenceAt", wire(reference_at)),
        ("validAt", wire(valid_at)),
    ]);
    identity(&object(fields), values)
}

/// compare one source object string with JavaScript strict equality
fn string_field_equals(value: &JsonValue, key: &str, expected: &str) -> bool {
    matches!(
        field(value, key),
        Some(JsonValue::String(actual)) if actual.iter().copied().eq(expected.encode_utf16())
    )
}

#[cfg(test)]
#[path = "wind_performance_counterfactual_test.rs"]
mod tests;
