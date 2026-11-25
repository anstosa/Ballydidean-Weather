//! Shared wire and evidence values for wind requalification.

use std::fmt::{self, Display};

use weather_contracts::{ForecastAdjustmentMetric, ForecastLeadBandKey, JsonValue};

use crate::MaintenanceCanonicalJson;
use crate::maintenance_canonical_json::{field, text};

/// One ordered member of the verified active wind runtime mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindPerformanceBand {
    pub metric: ForecastAdjustmentMetric,
    pub lead_band: ForecastLeadBandKey,
}

/// A retained predecessor diagnostic from the wind report boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindPerformanceError(String);

impl WindPerformanceError {
    /// retain an exact original failure before publishing any result
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for WindPerformanceError {
    /// render the closed phase or retained predecessor diagnostic
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for WindPerformanceError {}

/// Return the generated metric's original wire string.
pub(crate) fn metric_wire(metric: ForecastAdjustmentMetric) -> &'static str {
    ForecastAdjustmentMetric::WIRE_VALUES
        .iter()
        .find_map(|(wire, value)| (*value == metric).then_some(*wire))
        .expect("all generated metric values have exact wire spellings")
}

/// Return the generated lead band's original wire string.
pub(crate) fn lead_band_wire(band: ForecastLeadBandKey) -> &'static str {
    ForecastLeadBandKey::WIRE_VALUES
        .iter()
        .find_map(|(wire, value)| (*value == band).then_some(*wire))
        .expect("all generated lead bands have exact wire spellings")
}

/// Preserve the original metric-and-lead map identity.
pub(crate) fn band_key(band: &WindPerformanceBand) -> String {
    format!(
        "{}:{}",
        metric_wire(band.metric),
        lead_band_wire(band.lead_band)
    )
}

/// Bind the exact active definition into a canonical row preimage.
pub(crate) fn band_json(band: &WindPerformanceBand) -> JsonValue {
    object(vec![
        ("metric", wire(metric_wire(band.metric))),
        ("leadBand", wire(lead_band_wire(band.lead_band))),
    ])
}

/// Hash one exact canonical evidence preimage with the approved owner.
pub(crate) fn identity(
    value: &JsonValue,
    values: &mut impl MaintenanceCanonicalJson,
) -> Result<String, WindPerformanceError> {
    let bytes = values.encode_canonical_json(value).ok_or_else(|| {
        WindPerformanceError::new("wind performance identity serialization failed")
    })?;
    values
        .sha256(&bytes)
        .map(|digest| digest.as_str().to_owned())
        .ok_or_else(|| WindPerformanceError::new("wind performance identity digest unavailable"))
}

/// Require one present nonempty wire string.
pub(crate) fn required_text(
    value: &JsonValue,
    key: &str,
    description: &str,
) -> Result<String, WindPerformanceError> {
    text(value, key)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            WindPerformanceError::new(format!("{description} must be a nonempty string"))
        })
}

/// Require one finite wire number.
pub(crate) fn required_number(
    value: &JsonValue,
    key: &str,
    description: &str,
) -> Result<f64, WindPerformanceError> {
    // keep absence and nonfinite values distinct from admitted raw measurements
    match field(value, key) {
        Some(JsonValue::Number(number)) if number.is_finite() => Ok(*number),
        _ => Err(WindPerformanceError::new(format!(
            "{description} must be finite"
        ))),
    }
}

/// Require one original ordinary object.
pub(crate) fn require_object<'a>(
    value: &'a JsonValue,
    description: &str,
) -> Result<&'a JsonValue, WindPerformanceError> {
    // reject arrays and special scalar payloads
    if !matches!(value, JsonValue::Object(_)) {
        return Err(WindPerformanceError::new(format!(
            "{description} must be an object"
        )));
    }
    Ok(value)
}

/// Build an exact source wire string.
pub(crate) fn wire(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build an ordered source field value independent of serialization.
pub(crate) fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.encode_utf16().collect(), value))
            .collect(),
    )
}
