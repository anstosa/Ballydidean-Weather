//! Closed forecast evidence row projection over generated ordered JSON values.

use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::JsonValue;

use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant};

const ADJUSTED_METRICS: &[&str] = &[
    "apparentTemperatureC",
    "relativeHumidityPercent",
    "temperatureC",
    "windDirectionDegrees",
    "windGustMps",
    "windSpeedMps",
];
const RAW_METRICS: &[&str] = &[
    "precipitationMm",
    "temperatureC",
    "windGustMps",
    "windSpeedMps",
];
const SOURCE_DECISION_STATES: &[&str] = &["active", "disabled", "raw_fallback"];
const WIND_DECISION_STATES: &[&str] = &["active", "disabled", "not_applicable"];

/// closed projection consumed by the lead-owned capture and receipt assembly.
#[derive(Clone, Debug, PartialEq)]
pub struct ForecastProjectedEvidence {
    pub(crate) settings: JsonValue,
    pub(crate) bundle_identities: JsonValue,
    pub(crate) rows: Vec<JsonValue>,
    pub(crate) stable_rows: Vec<JsonValue>,
    pub(crate) availability: JsonValue,
}

/// one source-compatible forecast projection refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForecastEvidenceRowProjectionError {
    InvalidForecast,
    InvalidRow,
    InvalidValue(String),
    MustBePlainObject(String),
}

impl Display for ForecastEvidenceRowProjectionError {
    /// render the source projection's ordinary validation message.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // preserve the source's forecast, row, and field error categories
        match self {
            Self::InvalidForecast => formatter.write_str("forecast evidence body is invalid"),
            Self::InvalidRow => formatter.write_str("forecast evidence row is invalid"),
            Self::InvalidValue(path) => write!(formatter, "{path} is invalid"),
            Self::MustBePlainObject(path) => write!(formatter, "{path} must be a plain object"),
        }
    }
}

impl Error for ForecastEvidenceRowProjectionError {}

/// project one validated filtered forecast into scoring and stable row identities.
pub fn project_forecast_evidence_rows(
    forecast: &JsonValue,
    maximum_rows: usize,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<ForecastProjectedEvidence, ForecastEvidenceRowProjectionError> {
    let JsonValue::Object(forecast) = forecast else {
        return Err(ForecastEvidenceRowProjectionError::InvalidForecast);
    };
    let settings = field(forecast, "adjustmentSettings")
        .ok_or_else(|| invalid("forecast.adjustmentSettings"))?
        .clone();
    let Some(JsonValue::Array(source_rows)) = field(forecast, "data") else {
        return Err(ForecastEvidenceRowProjectionError::InvalidForecast);
    };

    // preserve the version-selected nonempty row bound
    if source_rows.is_empty() || source_rows.len() > maximum_rows {
        return Err(ForecastEvidenceRowProjectionError::InvalidForecast);
    }
    let bundle_identities = project_bundle_identities(forecast)?;
    let availability = project_availability(source_rows, time)?;
    let mut rows = Vec::with_capacity(source_rows.len());
    let mut stable_rows = Vec::with_capacity(source_rows.len());

    // preserve response order for rows and their stable identities
    for source_row in source_rows {
        let row = project_evidence_row(source_row, time)?;
        stable_rows.push(project_stable_row(&row)?);
        rows.push(row);
    }
    Ok(ForecastProjectedEvidence {
        settings,
        bundle_identities,
        rows,
        stable_rows,
        availability,
    })
}

/// retain only active response-level serving identities.
fn project_bundle_identities(
    forecast: &[(Vec<u16>, JsonValue)],
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    Ok(object([
        (
            "rain",
            object([(
                "activeBundle",
                nullable_sha256(nested(forecast, &["rainAdjustmentRuntime", "activeBundle"]))?,
            )]),
        ),
        (
            "temperature",
            object([
                (
                    "activeBundle",
                    nullable_sha256(nested(
                        forecast,
                        &["temperatureAdjustmentRuntime", "activeBundle"],
                    ))?,
                ),
                (
                    "authorizationSha256",
                    nullable_sha256(nested(
                        forecast,
                        &["temperatureAdjustmentRuntime", "authorizationSha256"],
                    ))?,
                ),
            ]),
        ),
        (
            "wind",
            object([
                (
                    "activeBundle",
                    nullable_sha256(nested(forecast, &["adjustmentRuntime", "activeBundle"]))?,
                ),
                (
                    "authorizationSha256",
                    nullable_sha256(nested(
                        forecast,
                        &["adjustmentRuntime", "authorizationSha256"],
                    ))?,
                ),
                (
                    "candidateArtifactSha256",
                    nullable_sha256(nested(
                        forecast,
                        &["adjustmentRuntime", "candidateArtifactSha256"],
                    ))?,
                ),
            ]),
        ),
    ]))
}

/// group mutable first-observed timestamps by ordered row index.
fn project_availability(
    rows: &[JsonValue],
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let mut timestamps: Vec<JsonValue> = Vec::new();
    let mut row_timestamp_indexes = Vec::with_capacity(rows.len());

    // retain each distinct mutable timestamp only once
    for row in rows {
        let received_at = nested_value(Some(row), &["receivedAt"]);
        let timestamp = required_instant(received_at, "row.receivedAt", time)?;
        let index = timestamps
            .iter()
            .position(|candidate| candidate == &timestamp)
            .unwrap_or_else(|| {
                let index = timestamps.len();
                timestamps.push(timestamp);
                index
            });
        row_timestamp_indexes.push(JsonValue::Number(index as f64));
    }
    Ok(object([
        (
            "rowTimestampIndexes",
            JsonValue::Array(row_timestamp_indexes),
        ),
        ("timestamps", JsonValue::Array(timestamps)),
    ]))
}

/// project one closed scoring row without volatile response metadata.
fn project_evidence_row(
    row: &JsonValue,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let JsonValue::Object(row) = row else {
        return Err(ForecastEvidenceRowProjectionError::InvalidRow);
    };
    let rain_adjustment = project_rain_adjustment(field(row, "rainAdjustment"), time)?;
    let raw = project_metric_values(field(row, "metrics"), RAW_METRICS, "row.metrics", false)?;
    let record = object([
        ("id", required_bounded_string(field(row, "id"), "row.id")?),
        (
            "productRunAt",
            nullable_instant(field(row, "productRunAt"), "row.productRunAt", time)?,
        ),
        (
            "revisionCount",
            required_revision_count(field(row, "revisionCount"))?,
        ),
        (
            "validAt",
            required_instant(field(row, "validAt"), "row.validAt", time)?,
        ),
    ]);
    let source = object([
        (
            "dataset",
            nullable_bounded_string(
                nested(row, &["metadata", "provider", "dataset"]),
                "row.metadata.provider.dataset",
            )?,
        ),
        (
            "providerKey",
            required_bounded_string(
                nested(row, &["provenance", "providerKey"]),
                "row.provenance.providerKey",
            )?,
        ),
        (
            "sourceId",
            required_bounded_string(
                nested(row, &["provenance", "sourceId"]),
                "row.provenance.sourceId",
            )?,
        ),
        (
            "sourceKey",
            required_bounded_string(
                nested(row, &["provenance", "sourceKey"]),
                "row.provenance.sourceKey",
            )?,
        ),
        (
            "upstreamModel",
            nullable_bounded_string(
                nested(row, &["metadata", "upstream", "model"]),
                "row.metadata.upstream.model",
            )?,
        ),
    ]);
    let temperature_adjustment =
        project_temperature_adjustment(field(row, "temperatureAdjustment"), time)?;
    let wind_adjustment = project_wind_adjustment(field(row, "adjustment"))?;
    Ok(object([
        ("provenanceComplete", JsonValue::Boolean(false)),
        ("rainAdjustment", rain_adjustment),
        ("raw", raw),
        ("record", record),
        ("source", source),
        ("temperatureAdjustment", temperature_adjustment),
        ("windAdjustment", wind_adjustment),
    ]))
}

/// retain post-settings generic wind decision fields only.
fn project_wind_adjustment(
    value: Option<&JsonValue>,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let decision = required_object(value, "row.adjustment")?;
    let adjusted_metrics = match field(decision, "adjustedMetrics") {
        None | Some(JsonValue::Null) => JsonValue::Object(Vec::new()),
        Some(value) => value.clone(),
    };
    let applied_metrics = match field(decision, "appliedMetrics") {
        None | Some(JsonValue::Null) => JsonValue::Array(Vec::new()),
        Some(value) => value.clone(),
    };
    Ok(object([
        (
            "adjustedMetrics",
            project_metric_values(
                Some(&adjusted_metrics),
                ADJUSTED_METRICS,
                "row.adjustment.adjustedMetrics",
                true,
            )?,
        ),
        (
            "appliedMetrics",
            required_string_array(Some(&applied_metrics), "row.adjustment.appliedMetrics")?,
        ),
        (
            "authorizationSha256",
            nullable_sha256(field(decision, "authorizationSha256"))?,
        ),
        (
            "candidateArtifactSha256",
            nullable_sha256(field(decision, "candidateArtifactSha256"))?,
        ),
        (
            "leadBand",
            nullable_bounded_string(field(decision, "leadBand"), "row.adjustment.leadBand")?,
        ),
        (
            "reasonCode",
            nullable_bounded_string(field(decision, "reasonCode"), "row.adjustment.reasonCode")?,
        ),
        (
            "state",
            required_state(
                field(decision, "state"),
                "row.adjustment.state",
                WIND_DECISION_STATES,
            )?,
        ),
    ]))
}

/// retain temperature amount, reason, and immutable source receipt fields.
fn project_temperature_adjustment(
    value: Option<&JsonValue>,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let decision = required_object(value, "row.temperatureAdjustment")?;
    Ok(object([
        (
            "branch",
            nullable_bounded_string(
                field(decision, "branch"),
                "row.temperatureAdjustment.branch",
            )?,
        ),
        (
            "bundleSha256",
            nullable_sha256(field(decision, "bundleSha256"))?,
        ),
        (
            "correctedTemperatureC",
            nullable_finite(
                field(decision, "correctedTemperatureC"),
                "row.temperatureAdjustment.correctedTemperatureC",
            )?,
        ),
        (
            "rawBestMatchTemperatureC",
            nullable_finite(
                field(decision, "rawBestMatchTemperatureC"),
                "row.temperatureAdjustment.rawBestMatchTemperatureC",
            )?,
        ),
        (
            "reasonCode",
            nullable_bounded_string(
                field(decision, "reasonCode"),
                "row.temperatureAdjustment.reasonCode",
            )?,
        ),
        (
            "sourceForecast",
            project_temperature_source(field(decision, "sourceForecast"), time)?,
        ),
        (
            "state",
            required_state(
                field(decision, "state"),
                "row.temperatureAdjustment.state",
                SOURCE_DECISION_STATES,
            )?,
        ),
    ]))
}

/// retain only the closed temperature source forecast.
fn project_temperature_source(
    value: Option<&JsonValue>,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(value) = non_null(value) else {
        return Ok(JsonValue::Null);
    };
    let source = required_object(Some(value), "row.temperatureAdjustment.sourceForecast")?;
    Ok(object([
        (
            "adapterVersion",
            required_bounded_string(
                field(source, "adapterVersion"),
                "temperature.source.adapterVersion",
            )?,
        ),
        (
            "dataset",
            required_bounded_string(field(source, "dataset"), "temperature.source.dataset")?,
        ),
        (
            "firstReceivedAt",
            required_instant(
                field(source, "firstReceivedAt"),
                "temperature.source.firstReceivedAt",
                time,
            )?,
        ),
        (
            "modelCycle",
            required_bounded_string(field(source, "modelCycle"), "temperature.source.modelCycle")?,
        ),
        (
            "modelLeadHours",
            required_bounded_integer(
                field(source, "modelLeadHours"),
                "temperature.source.modelLeadHours",
            )?,
        ),
        (
            "operationalHorizonHours",
            required_bounded_integer(
                field(source, "operationalHorizonHours"),
                "temperature.source.operationalHorizonHours",
            )?,
        ),
        (
            "providerKey",
            required_bounded_string(
                field(source, "providerKey"),
                "temperature.source.providerKey",
            )?,
        ),
        (
            "providerResponseSha256",
            required_sha256(
                field(source, "providerResponseSha256"),
                "temperature.source.providerResponseSha256",
            )?,
        ),
        (
            "rawRelativeHumidityPercent",
            nullable_finite(
                field(source, "rawRelativeHumidityPercent"),
                "temperature.source.rawRelativeHumidityPercent",
            )?,
        ),
        (
            "rawTemperatureC",
            required_finite(
                field(source, "rawTemperatureC"),
                "temperature.source.rawTemperatureC",
            )?,
        ),
        (
            "rawWindSpeedMps",
            nullable_finite(
                field(source, "rawWindSpeedMps"),
                "temperature.source.rawWindSpeedMps",
            )?,
        ),
        (
            "runInitializedAt",
            required_instant(
                field(source, "runInitializedAt"),
                "temperature.source.runInitializedAt",
                time,
            )?,
        ),
        (
            "upstreamModel",
            required_bounded_string(
                field(source, "upstreamModel"),
                "temperature.source.upstreamModel",
            )?,
        ),
        (
            "validAt",
            required_instant(field(source, "validAt"), "temperature.source.validAt", time)?,
        ),
    ]))
}

/// retain rain amount, reason, and immutable source receipt fields.
fn project_rain_adjustment(
    value: Option<&JsonValue>,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let decision = required_object(value, "row.rainAdjustment")?;
    Ok(object([
        (
            "bundleSha256",
            nullable_sha256(field(decision, "bundleSha256"))?,
        ),
        (
            "correctedPrecipitationMm",
            nullable_finite(
                field(decision, "correctedPrecipitationMm"),
                "row.rainAdjustment.correctedPrecipitationMm",
            )?,
        ),
        (
            "rawBestMatchPrecipitationMm",
            nullable_finite(
                field(decision, "rawBestMatchPrecipitationMm"),
                "row.rainAdjustment.rawBestMatchPrecipitationMm",
            )?,
        ),
        (
            "reasonCode",
            nullable_bounded_string(
                field(decision, "reasonCode"),
                "row.rainAdjustment.reasonCode",
            )?,
        ),
        (
            "sourceForecast",
            project_rain_source(field(decision, "sourceForecast"), time)?,
        ),
        (
            "state",
            required_state(
                field(decision, "state"),
                "row.rainAdjustment.state",
                SOURCE_DECISION_STATES,
            )?,
        ),
    ]))
}

/// retain only the closed rain source forecast.
fn project_rain_source(
    value: Option<&JsonValue>,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(value) = non_null(value) else {
        return Ok(JsonValue::Null);
    };
    let source = required_object(Some(value), "row.rainAdjustment.sourceForecast")?;
    Ok(object([
        (
            "decisionAt",
            required_instant(field(source, "decisionAt"), "rain.source.decisionAt", time)?,
        ),
        (
            "firstReceivedAt",
            required_instant(
                field(source, "firstReceivedAt"),
                "rain.source.firstReceivedAt",
                time,
            )?,
        ),
        (
            "modelLeadHours",
            required_bounded_integer(
                field(source, "modelLeadHours"),
                "rain.source.modelLeadHours",
            )?,
        ),
        (
            "providerKey",
            required_bounded_string(field(source, "providerKey"), "rain.source.providerKey")?,
        ),
        (
            "rawPrecipitationMm",
            required_finite(
                field(source, "rawPrecipitationMm"),
                "rain.source.rawPrecipitationMm",
            )?,
        ),
        (
            "runInitializedAt",
            required_instant(
                field(source, "runInitializedAt"),
                "rain.source.runInitializedAt",
                time,
            )?,
        ),
        (
            "upstreamModel",
            required_bounded_string(field(source, "upstreamModel"), "rain.source.upstreamModel")?,
        ),
        (
            "validAt",
            required_instant(field(source, "validAt"), "rain.source.validAt", time)?,
        ),
    ]))
}

/// retain a fixed metric allowlist with nulls for absent raw values.
fn project_metric_values(
    value: Option<&JsonValue>,
    keys: &[&str],
    path: &str,
    omit_absent: bool,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let source = required_object(value, path)?;
    let mut projected = Vec::with_capacity(keys.len());

    // preserve the reviewed metric order
    for key in keys {
        let metric = field(source, key);

        // omit only absent adjusted metrics
        if !omit_absent || metric.is_some() {
            projected.push((
                utf16(key),
                nullable_finite(metric, &format!("{path}.{key}"))?,
            ));
        }
    }
    Ok(JsonValue::Object(projected))
}

/// derive the scoring-independent stable identity from one projected row.
fn project_stable_row(row: &JsonValue) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let row = required_object(Some(row), "forecast evidence row")?;
    Ok(object([
        (
            "record",
            field(row, "record")
                .ok_or_else(|| invalid("row.record"))?
                .clone(),
        ),
        (
            "source",
            field(row, "source")
                .ok_or_else(|| invalid("row.source"))?
                .clone(),
        ),
    ]))
}

/// require one object value at a known source path.
fn required_object<'a>(
    value: Option<&'a JsonValue>,
    path: &str,
) -> Result<&'a [(Vec<u16>, JsonValue)], ForecastEvidenceRowProjectionError> {
    let Some(JsonValue::Object(value)) = value else {
        return Err(ForecastEvidenceRowProjectionError::MustBePlainObject(
            path.to_owned(),
        ));
    };
    Ok(value)
}

/// require one bounded opaque string while preserving UTF-16 identity.
fn required_bounded_string(
    value: Option<&JsonValue>,
    path: &str,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(JsonValue::String(value)) = value else {
        return Err(invalid(path));
    };

    // preserve JavaScript length and control-code bounds
    if value.is_empty()
        || value.len() > 200
        || value
            .iter()
            .any(|unit| matches!(*unit, 0x0000..=0x001f | 0x007f))
    {
        return Err(invalid(path));
    }
    Ok(JsonValue::String(value.clone()))
}

/// normalize one absent bounded string to null.
fn nullable_bounded_string(
    value: Option<&JsonValue>,
    path: &str,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(value) = non_null(value) else {
        return Ok(JsonValue::Null);
    };
    required_bounded_string(Some(value), path)
}

/// require one bounded string array in source order.
fn required_string_array(
    value: Option<&JsonValue>,
    path: &str,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(JsonValue::Array(values)) = value else {
        return Err(invalid(path));
    };

    // preserve the source's bounded metric name list
    if values.len() > 16 {
        return Err(invalid(path));
    }
    let mut projected = Vec::with_capacity(values.len());

    // validate every metric name
    for value in values {
        projected.push(required_bounded_string(Some(value), &format!("{path}[]"))?);
    }
    Ok(JsonValue::Array(projected))
}

/// require one nonnegative bounded record revision.
fn required_revision_count(
    value: Option<&JsonValue>,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(JsonValue::Number(value)) = value else {
        return Err(invalid("row.revisionCount"));
    };

    // preserve the source safe integer and revision bounds
    if !is_integer(*value) || *value < 0.0 || *value > 1_000_000.0 {
        return Err(invalid("row.revisionCount"));
    }
    Ok(JsonValue::Number(*value))
}

/// require one bounded signed integer.
fn required_bounded_integer(
    value: Option<&JsonValue>,
    path: &str,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(JsonValue::Number(value)) = value else {
        return Err(invalid(path));
    };

    // preserve the source safe integer and magnitude bounds
    if !is_integer(*value) || value.abs() > 1_000_000.0 {
        return Err(invalid(path));
    }
    Ok(JsonValue::Number(*value))
}

/// require one finite number.
fn required_finite(
    value: Option<&JsonValue>,
    path: &str,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(JsonValue::Number(value)) = value else {
        return Err(invalid(path));
    };

    // reject nonfinite generated storage values
    if !value.is_finite() {
        return Err(invalid(path));
    }
    Ok(JsonValue::Number(*value))
}

/// normalize one absent finite number to null.
fn nullable_finite(
    value: Option<&JsonValue>,
    path: &str,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(value) = non_null(value) else {
        return Ok(JsonValue::Null);
    };
    required_finite(Some(value), path)
}

/// require one exact family-specific public state.
fn required_state(
    value: Option<&JsonValue>,
    path: &str,
    states: &[&str],
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(JsonValue::String(value)) = value else {
        return Err(invalid(path));
    };

    // reject unknown and cross-family states
    if !states.iter().any(|state| utf16_equals(value, state)) {
        return Err(invalid(path));
    }
    Ok(JsonValue::String(value.clone()))
}

/// require one lowercase SHA-256 value.
fn required_sha256(
    value: Option<&JsonValue>,
    path: &str,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(JsonValue::String(value)) = value else {
        return Err(invalid(path));
    };

    // preserve the exact lowercase hexadecimal digest alphabet
    if value.len() != 64
        || !value.iter().all(|unit| {
            matches!(u8::try_from(*unit), Ok(byte) if byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    {
        return Err(invalid(path));
    }
    Ok(JsonValue::String(value.clone()))
}

/// normalize one absent SHA-256 identity to null.
fn nullable_sha256(
    value: Option<&JsonValue>,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(value) = non_null(value) else {
        return Ok(JsonValue::Null);
    };
    required_sha256(Some(value), "sha256")
}

/// require one canonical UTC instant through the admitted codec.
fn required_instant(
    value: Option<&JsonValue>,
    path: &str,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(JsonValue::String(value)) = value else {
        return Err(invalid(path));
    };
    let decoded = String::from_utf16(value).map_err(|_| invalid(path))?;

    // delegate strict canonical UTC admission to the existing codec boundary
    if instant(time, &decoded).is_none() {
        return Err(invalid(path));
    }
    Ok(JsonValue::String(value.clone()))
}

/// normalize one absent canonical UTC instant to null.
fn nullable_instant(
    value: Option<&JsonValue>,
    path: &str,
    time: &impl RevisionArchiveCanonicalCodec,
) -> Result<JsonValue, ForecastEvidenceRowProjectionError> {
    let Some(value) = non_null(value) else {
        return Ok(JsonValue::Null);
    };
    required_instant(Some(value), path, time)
}

/// return a non-null optional value.
fn non_null(value: Option<&JsonValue>) -> Option<&JsonValue> {
    // normalize missing and explicit null together
    match value {
        None | Some(JsonValue::Null) => None,
        Some(value) => Some(value),
    }
}

/// find one property on an already-admitted ordered object.
fn field<'a>(object: &'a [(Vec<u16>, JsonValue)], key: &str) -> Option<&'a JsonValue> {
    object
        .iter()
        .find(|(candidate, _)| utf16_equals(candidate, key))
        .map(|(_, value)| value)
}

/// follow optional JavaScript-style property access through ordered objects.
fn nested<'a>(object: &'a [(Vec<u16>, JsonValue)], path: &[&str]) -> Option<&'a JsonValue> {
    let (first, remaining) = path.split_first()?;
    let mut value = field(object, first);

    // continue optional access only through object values
    for key in remaining {
        let Some(JsonValue::Object(next)) = value else {
            return None;
        };
        value = field(next, key);
    }
    value
}

/// follow optional JavaScript-style property access from any value.
fn nested_value<'a>(mut value: Option<&'a JsonValue>, path: &[&str]) -> Option<&'a JsonValue> {
    // stop optional access on missing, null, or nonobject values
    for key in path {
        let Some(JsonValue::Object(object)) = value else {
            return None;
        };
        value = field(object, key);
    }
    value
}

/// construct one ordered generated JSON object.
fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (utf16(key), value))
            .collect(),
    )
}

/// encode one field or fixture string into generated UTF-16 storage.
#[cfg(test)]
fn text(value: &str) -> JsonValue {
    JsonValue::String(utf16(value))
}

/// encode one property identity into generated UTF-16 storage.
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// compare generated UTF-16 storage without allocating a decoded string.
fn utf16_equals(value: &[u16], expected: &str) -> bool {
    value.iter().copied().eq(expected.encode_utf16())
}

/// report one exact source field validation failure.
fn invalid(path: &str) -> ForecastEvidenceRowProjectionError {
    ForecastEvidenceRowProjectionError::InvalidValue(path.to_owned())
}

/// recognize one JavaScript-safe integer within the tighter source bounds.
fn is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixtureTime;

    impl RevisionArchiveCanonicalCodec for FixtureTime {
        /// admit only source-derived canonical fixture instants.
        fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
            // keep timestamp parsing at the admitted test boundary
            match text {
                "2026-11-23T12:00:00.000Z" => Some(1_795_435_200_000),
                "2026-11-23T15:10:00.000Z" => Some(1_795_446_600_000),
                "2026-11-23T15:30:00.000Z" => Some(1_795_447_800_000),
                "2026-11-23T15:40:00.000Z" => Some(1_795_448_400_000),
                "2026-11-23T16:00:00.000Z" => Some(1_795_449_600_000),
                _ => None,
            }
        }

        /// reject accidental hashing in the projection-only lane.
        fn sha256(&self, _bytes: &[u8]) -> [u8; 32] {
            unreachable!("projection does not own hashing")
        }
    }

    /// build one lowercase fixture digest.
    fn digest(character: char) -> JsonValue {
        text(&character.to_string().repeat(64))
    }

    /// build one active wind decision with extra unprojected material.
    fn wind_adjustment() -> JsonValue {
        object([
            (
                "adjustedMetrics",
                object([
                    ("temperatureC", JsonValue::Number(11.5)),
                    ("windSpeedMps", JsonValue::Number(4.2)),
                    ("ignoredMetric", JsonValue::Number(99.0)),
                ]),
            ),
            (
                "appliedMetrics",
                JsonValue::Array(vec![text("temperatureC"), text("windSpeedMps")]),
            ),
            ("authorizationSha256", digest('d')),
            ("candidateArtifactSha256", digest('e')),
            ("leadBand", text("h006_h012")),
            ("reasonCode", text("active_bundle")),
            ("state", text("active")),
            ("volatile", text("discarded")),
        ])
    }

    /// build one active temperature decision and immutable source forecast.
    fn temperature_adjustment() -> JsonValue {
        object([
            ("branch", text("adaptive")),
            ("bundleSha256", digest('b')),
            ("correctedTemperatureC", JsonValue::Number(11.5)),
            ("rawBestMatchTemperatureC", JsonValue::Number(10.0)),
            ("reasonCode", text("active_bundle")),
            (
                "sourceForecast",
                object([
                    ("adapterVersion", text("ecmwf-temperature-v1")),
                    ("dataset", text("ifs")),
                    ("firstReceivedAt", text("2026-11-23T15:30:00.000Z")),
                    ("modelCycle", text("12z")),
                    ("modelLeadHours", JsonValue::Number(4.0)),
                    ("operationalHorizonHours", JsonValue::Number(240.0)),
                    ("providerKey", text("open-meteo")),
                    ("providerResponseSha256", digest('1')),
                    ("rawRelativeHumidityPercent", JsonValue::Number(82.0)),
                    ("rawTemperatureC", JsonValue::Number(10.0)),
                    ("rawWindSpeedMps", JsonValue::Number(3.0)),
                    ("runInitializedAt", text("2026-11-23T12:00:00.000Z")),
                    ("upstreamModel", text("ecmwf_ifs")),
                    ("validAt", text("2026-11-23T16:00:00.000Z")),
                ]),
            ),
            ("state", text("active")),
        ])
    }

    /// build one active rain decision and immutable source forecast.
    fn rain_adjustment() -> JsonValue {
        object([
            ("bundleSha256", digest('c')),
            ("correctedPrecipitationMm", JsonValue::Number(0.7)),
            ("rawBestMatchPrecipitationMm", JsonValue::Number(0.4)),
            ("reasonCode", text("active_bundle")),
            (
                "sourceForecast",
                object([
                    ("decisionAt", text("2026-11-23T15:10:00.000Z")),
                    ("firstReceivedAt", text("2026-11-23T15:30:00.000Z")),
                    ("modelLeadHours", JsonValue::Number(4.0)),
                    ("providerKey", text("open-meteo")),
                    ("rawPrecipitationMm", JsonValue::Number(0.4)),
                    ("runInitializedAt", text("2026-11-23T12:00:00.000Z")),
                    ("upstreamModel", text("ecmwf_ifs")),
                    ("validAt", text("2026-11-23T16:00:00.000Z")),
                ]),
            ),
            ("state", text("active")),
        ])
    }

    /// build one complete ordinary forecast row.
    fn row(id: &str, received_at: &str) -> JsonValue {
        object([
            ("id", text(id)),
            ("productRunAt", text("2026-11-23T12:00:00.000Z")),
            ("receivedAt", text(received_at)),
            ("revisionCount", JsonValue::Number(2.0)),
            ("validAt", text("2026-11-23T16:00:00.000Z")),
            (
                "metrics",
                object([
                    ("precipitationMm", JsonValue::Number(0.4)),
                    ("temperatureC", JsonValue::Number(10.0)),
                    ("windSpeedMps", JsonValue::Number(3.0)),
                    ("ignoredMetric", JsonValue::Number(99.0)),
                ]),
            ),
            (
                "metadata",
                object([
                    ("provider", object([("dataset", text("ifs"))])),
                    ("upstream", object([("model", text("ecmwf_ifs"))])),
                    ("volatile", text("discarded")),
                ]),
            ),
            (
                "provenance",
                object([
                    ("providerKey", text("open-meteo")),
                    ("sourceId", text("source-ecmwf")),
                    ("sourceKey", text("ecmwf")),
                ]),
            ),
            ("adjustment", wind_adjustment()),
            ("temperatureAdjustment", temperature_adjustment()),
            ("rainAdjustment", rain_adjustment()),
        ])
    }

    /// build one filtered response with repeated and distinct availability times.
    fn forecast() -> JsonValue {
        object([
            (
                "adjustmentSettings",
                object([
                    ("rain", JsonValue::Boolean(true)),
                    ("temperature", JsonValue::Boolean(true)),
                    ("wind", JsonValue::Boolean(true)),
                ]),
            ),
            (
                "rainAdjustmentRuntime",
                object([("activeBundle", digest('a'))]),
            ),
            (
                "temperatureAdjustmentRuntime",
                object([
                    ("activeBundle", digest('b')),
                    ("authorizationSha256", digest('c')),
                ]),
            ),
            (
                "adjustmentRuntime",
                object([
                    ("activeBundle", digest('d')),
                    ("authorizationSha256", digest('e')),
                    ("candidateArtifactSha256", digest('f')),
                ]),
            ),
            (
                "data",
                JsonValue::Array(vec![
                    row("row-1", "2026-11-23T15:30:00.000Z"),
                    row("row-2", "2026-11-23T15:30:00.000Z"),
                    row("row-3", "2026-11-23T15:40:00.000Z"),
                ]),
            ),
        ])
    }

    /// decode ordered field names for exact projection-order assertions.
    fn keys(value: &JsonValue) -> Vec<String> {
        let JsonValue::Object(value) = value else {
            panic!("expected object")
        };
        value
            .iter()
            .map(|(key, _)| String::from_utf16(key).expect("fixture key"))
            .collect()
    }

    /// prove active row values, stable identities, and mutable availability projection.
    #[test]
    fn projects_active_rows_and_deduplicates_mutable_availability() {
        let forecast = forecast();
        let projected = project_forecast_evidence_rows(&forecast, 240, &FixtureTime)
            .expect("ordinary evidence projection");

        assert_eq!(
            projected.settings,
            object([
                ("rain", JsonValue::Boolean(true)),
                ("temperature", JsonValue::Boolean(true)),
                ("wind", JsonValue::Boolean(true)),
            ])
        );
        assert_eq!(
            projected.bundle_identities,
            object([
                ("rain", object([("activeBundle", digest('a'))])),
                (
                    "temperature",
                    object([
                        ("activeBundle", digest('b')),
                        ("authorizationSha256", digest('c')),
                    ]),
                ),
                (
                    "wind",
                    object([
                        ("activeBundle", digest('d')),
                        ("authorizationSha256", digest('e')),
                        ("candidateArtifactSha256", digest('f')),
                    ]),
                ),
            ])
        );
        assert_eq!(
            projected.availability,
            object([
                (
                    "rowTimestampIndexes",
                    JsonValue::Array(vec![
                        JsonValue::Number(0.0),
                        JsonValue::Number(0.0),
                        JsonValue::Number(1.0),
                    ]),
                ),
                (
                    "timestamps",
                    JsonValue::Array(vec![
                        text("2026-11-23T15:30:00.000Z"),
                        text("2026-11-23T15:40:00.000Z"),
                    ]),
                ),
            ])
        );
        assert_eq!(projected.rows.len(), 3);
        assert_eq!(projected.stable_rows.len(), 3);
        assert_eq!(
            keys(&projected.rows[0]),
            [
                "provenanceComplete",
                "rainAdjustment",
                "raw",
                "record",
                "source",
                "temperatureAdjustment",
                "windAdjustment",
            ]
        );
        let JsonValue::Object(first_row) = &projected.rows[0] else {
            panic!("projected row")
        };
        assert_eq!(
            field(first_row, "raw"),
            Some(&object([
                ("precipitationMm", JsonValue::Number(0.4)),
                ("temperatureC", JsonValue::Number(10.0)),
                ("windGustMps", JsonValue::Null),
                ("windSpeedMps", JsonValue::Number(3.0)),
            ]))
        );
        assert_eq!(
            nested(
                first_row,
                &["temperatureAdjustment", "correctedTemperatureC"]
            ),
            Some(&JsonValue::Number(11.5))
        );
        assert_eq!(
            nested(first_row, &["rainAdjustment", "correctedPrecipitationMm"]),
            Some(&JsonValue::Number(0.7))
        );
        assert_eq!(
            nested(first_row, &["windAdjustment", "adjustedMetrics"]),
            Some(&object([
                ("temperatureC", JsonValue::Number(11.5)),
                ("windSpeedMps", JsonValue::Number(4.2)),
            ]))
        );
        assert!(field(first_row, "receivedAt").is_none());
        assert_eq!(keys(&projected.stable_rows[0]), ["record", "source"]);
        let JsonValue::Object(stable_row) = &projected.stable_rows[0] else {
            panic!("stable row")
        };
        assert_eq!(field(stable_row, "record"), field(first_row, "record"));
        assert_eq!(field(stable_row, "source"), field(first_row, "source"));
    }

    /// prove row bounds are rejected before any projection work.
    #[test]
    fn rejects_a_forecast_above_the_selected_version_bound() {
        assert_eq!(
            project_forecast_evidence_rows(&forecast(), 2, &FixtureTime),
            Err(ForecastEvidenceRowProjectionError::InvalidForecast)
        );
    }

    /// prove mutable availability timestamps use the admitted canonical time owner.
    #[test]
    fn rejects_a_noncanonical_availability_timestamp() {
        let mut forecast = forecast();
        let JsonValue::Object(forecast_fields) = &mut forecast else {
            unreachable!("fixture forecast")
        };
        let Some(JsonValue::Array(rows)) = forecast_fields
            .iter_mut()
            .find(|(key, _)| utf16_equals(key, "data"))
            .map(|(_, value)| value)
        else {
            unreachable!("fixture rows")
        };
        let JsonValue::Object(first_row) = &mut rows[0] else {
            unreachable!("fixture row")
        };
        let received_at = first_row
            .iter_mut()
            .find(|(key, _)| utf16_equals(key, "receivedAt"))
            .map(|(_, value)| value)
            .expect("receivedAt");
        *received_at = text("2026-11-23T15:30:00Z");

        assert_eq!(
            project_forecast_evidence_rows(&forecast, 240, &FixtureTime),
            Err(ForecastEvidenceRowProjectionError::InvalidValue(
                "row.receivedAt".to_owned(),
            ))
        );
    }
}
