use std::collections::HashMap;

use weather_contracts::{
    ApiAirQualityHourlyContext, ApiCurrentWeatherRecord, ApiCurrentWeatherResponse,
    ApiPropertySensorSnapshot, ApiSite, ApiSource, ApiWeatherRecordFreshness,
    ApiWeatherRecordFreshnessStatusEnum, ApiWeatherRecordMetadata, ApiWeatherRecordMetadataDevice,
    ApiWeatherRecordMetadataProvider, ApiWeatherRecordMetadataQuality,
    ApiWeatherRecordMetadataUpstream, ApiWeatherRecordMetrics, ApiWeatherRecordProvenance,
    ApiWeatherRecordProvenanceAttribution, CurrentWeatherRecordRow, FieldPresence, JsDateOrString,
    JsonValue, Nullable, OrderedFiniteNumberRecord, SourceKind,
};

/// ordered particulate windows by sensor
type SensorHourlyValues = Nullable<Vec<(Vec<u16>, Vec<Nullable<f64>>)>>;

/// current-only sensor particulate inputs
#[derive(Clone, Copy)]
pub(crate) struct SensorAirQualityContext<'a> {
    hourly_by_sensor: &'a SensorHourlyValues,
    through: &'a JsDateOrString,
}

/// bounded current projection failure
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentProjectionError {
    pub code: &'static str,
    pub detail: String,
}

/// canonical instant from the time adapter
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalInstant {
    pub epoch_milliseconds: i64,
    pub utc: String,
}

/// resolve repository times outside this crate
pub trait CurrentTimeResolver {
    /// resolve one repository time
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError>;
}

/// project the canonical current response
pub fn project_current_weather_response(
    rows: Vec<CurrentWeatherRecordRow>,
    site: ApiSite,
    data_at: CanonicalInstant,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<ApiCurrentWeatherResponse, CurrentProjectionError> {
    let sources = index_sources(&site);
    let generated_at = data_at.utc.clone();
    let mut data = Vec::with_capacity(rows.len());

    // preserve source-aligned repository order
    for row in rows {
        let source = sources
            .get(row.sourceId.as_str())
            .ok_or_else(|| CurrentProjectionError {
                code: "current_projection.source",
                detail: format!("Missing active metadata for source {}", row.sourceId),
            })?;
        let valid_at = time_resolver.resolve(&row.validAt)?;
        let received_at = time_resolver.resolve(&row.lastReceivedAt)?.utc;
        let product_run_at = map_nullable_instant(&row.productRunAt, time_resolver)?;
        let air_quality = project_air_quality(
            &row.pm25HourlyConcentrations,
            &row.pm25HourlyThrough,
            time_resolver,
        )?;
        let metadata = project_metadata(&row, time_resolver)?;
        let freshness = describe_freshness(valid_at.epoch_milliseconds, data_at.epoch_milliseconds);
        let metrics = project_metrics(&row);
        let provenance = ApiWeatherRecordProvenance {
            attribution: ApiWeatherRecordProvenanceAttribution {
                label: source.attribution.label.clone(),
                url: source.attribution.url.clone(),
            },
            label: source_kind_label(row.sourceKind).to_owned(),
            providerKey: source.providerKey.clone(),
            sourceId: row.sourceId,
            sourceKey: source.key.clone(),
            sourceKind: row.sourceKind,
            stationSlug: row.stationSlug,
        };

        data.push(ApiCurrentWeatherRecord {
            freshness,
            id: row.id,
            metadata,
            metrics,
            productRunAt: product_run_at,
            provenance,
            receivedAt: received_at,
            revisionCount: row.revisionCount,
            validAt: valid_at.utc,
            airQuality: air_quality,
            pressureChange3hHpa: row.pressureChange3hHpa,
        });
    }

    Ok(ApiCurrentWeatherResponse {
        data,
        generatedAt: generated_at,
        site,
    })
}

/// index active source metadata
pub(crate) fn index_sources(site: &ApiSite) -> HashMap<&str, &ApiSource> {
    let mut sources = HashMap::new();

    // visit every station in canonical order
    for station in &site.stations {
        // visit every source in canonical order
        for source in &station.sources {
            sources.insert(source.id.as_str(), source);
        }
    }

    sources
}

/// project public record metadata
fn project_metadata(
    row: &CurrentWeatherRecordRow,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<ApiWeatherRecordMetadata, CurrentProjectionError> {
    // omit fully absent device metadata
    let device = match (&row.deviceVendor, &row.deviceModel, &row.deviceSerial) {
        (Nullable::Null, Nullable::Null, Nullable::Null) => Nullable::Null,
        _ => Nullable::Value(ApiWeatherRecordMetadataDevice {
            model: row.deviceModel.clone(),
            serial: row.deviceSerial.clone(),
            vendor: row.deviceVendor.clone(),
        }),
    };
    let quality = project_quality(&row.qualityMetadata)?;
    let provider = project_provider(
        &row.providerMetadata,
        Some(SensorAirQualityContext {
            hourly_by_sensor: &row.pm25HourlyBySensor,
            through: &row.pm25HourlyThrough,
        }),
        time_resolver,
    )?;

    Ok(ApiWeatherRecordMetadata {
        device,
        provider,
        quality,
        upstream: ApiWeatherRecordMetadataUpstream {
            model: row.upstreamModel.clone(),
            timezone: row.upstreamTimezone.clone(),
        },
    })
}

/// project allowlisted quality metadata
pub(crate) fn project_quality(
    metadata: &Nullable<Vec<(Vec<u16>, JsonValue)>>,
) -> Result<Nullable<ApiWeatherRecordMetadataQuality>, CurrentProjectionError> {
    let Nullable::Value(metadata) = metadata else {
        return Ok(Nullable::Null);
    };

    Ok(Nullable::Value(ApiWeatherRecordMetadataQuality {
        confidencePercent: bounded_number(object_value(metadata, "confidence_percent")),
        flags: bounded_strings(object_value(metadata, "flags"))?,
        interpolation: bounded_string(object_value(metadata, "interpolation"), 256)?,
        status: bounded_string(object_value(metadata, "status"), 256)?,
    }))
}

/// project allowlisted provider metadata
pub(crate) fn project_provider(
    metadata: &Nullable<Vec<(Vec<u16>, JsonValue)>>,
    sensor_air_quality: Option<SensorAirQualityContext<'_>>,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Nullable<ApiWeatherRecordMetadataProvider>, CurrentProjectionError> {
    let Nullable::Value(metadata) = metadata else {
        return Ok(Nullable::Null);
    };

    Ok(Nullable::Value(ApiWeatherRecordMetadataProvider {
        dataset: bounded_string(object_value(metadata, "dataset"), 256)?,
        elevationM: bounded_number(object_value(metadata, "elevation_m")),
        gridCell: bounded_string(object_value(metadata, "grid_cell"), 256)?,
        propertySensors: project_property_sensors(
            object_value(metadata, "property_sensors"),
            sensor_air_quality,
            time_resolver,
        )?,
    }))
}

/// project bounded property sensors
fn project_property_sensors(
    value: Option<&JsonValue>,
    sensor_air_quality: Option<SensorAirQualityContext<'_>>,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Nullable<Vec<ApiPropertySensorSnapshot>>, CurrentProjectionError> {
    let Some(JsonValue::Array(candidates)) = value else {
        return Ok(Nullable::Null);
    };
    let mut sensors = Vec::new();

    // retain only the first thirty-two sensor candidates
    for candidate in candidates.iter().take(32) {
        let JsonValue::Object(candidate) = candidate else {
            continue;
        };
        let key = bounded_string(object_value(candidate, "key"), 256)?;
        let model = bounded_string(object_value(candidate, "model"), 256)?;
        let channel = project_channel(object_value(candidate, "channel"));

        // require stable sensor identity and an optional bounded channel
        let (Nullable::Value(key), Nullable::Value(model), Some(channel)) = (key, model, channel)
        else {
            continue;
        };

        // enforce the source-owned lowercase kebab key grammar
        if !is_sensor_key(&key) {
            continue;
        }
        // expose particulate context only for current rows
        let air_quality = match sensor_air_quality {
            Some(context) => {
                let values = sensor_hourly_values(context.hourly_by_sensor, &key)?;
                FieldPresence::Value(project_air_quality_values(
                    values,
                    context.through,
                    time_resolver,
                )?)
            }
            None => FieldPresence::Missing,
        };
        let readings = project_sensor_readings(object_value(candidate, "readings"))?;

        sensors.push(ApiPropertySensorSnapshot {
            airQuality: air_quality,
            channel,
            key,
            model,
            readings,
        });
    }

    Ok(Nullable::Value(sensors))
}

/// project one sensor channel
fn project_channel(value: Option<&JsonValue>) -> Option<Nullable<u64>> {
    // collapse absent and nonnumeric channels to public null
    let Some(JsonValue::Number(number)) = value else {
        return Some(Nullable::Null);
    };

    // collapse nonfinite channels like node
    if !number.is_finite() {
        return Some(Nullable::Null);
    }

    // reject finite numeric channels outside the safe public range
    if number.fract() != 0.0 || !(1.0..=32.0).contains(number) {
        return None;
    }

    Some(Nullable::Value(*number as u64))
}

/// validate one sensor key
fn is_sensor_key(value: &str) -> bool {
    let mut segments = value.split('-');

    // require every segment to contain lowercase ASCII letters or digits
    segments.all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

/// project ordered sensor readings
fn project_sensor_readings(
    value: Option<&JsonValue>,
) -> Result<OrderedFiniteNumberRecord, CurrentProjectionError> {
    let Some(JsonValue::Object(entries)) = value else {
        return Ok(OrderedFiniteNumberRecord(Vec::new()));
    };
    let mut readings = Vec::new();

    // retain only the first twenty-four metric entries
    for (key, value) in entries.iter().take(24) {
        let key = utf16_string(key)?;
        let JsonValue::Number(value) = value else {
            continue;
        };

        // retain finite values with bounded camel-case keys
        if is_metric_key(&key) && value.is_finite() {
            readings.push((key, *value));
        }
    }

    Ok(OrderedFiniteNumberRecord(readings))
}

/// locate one sensor window
fn sensor_hourly_values<'a>(
    values: &'a SensorHourlyValues,
    key: &str,
) -> Result<Option<&'a Vec<Nullable<f64>>>, CurrentProjectionError> {
    let Nullable::Value(values) = values else {
        return Ok(None);
    };

    // retain the first matching ordered sensor key
    for (candidate, hourly) in values {
        // compare one decoded sensor key
        if utf16_string(candidate)? == key {
            return Ok(Some(hourly));
        }
    }

    Ok(None)
}

/// project aggregate particulate context
fn project_air_quality(
    values: &Nullable<Vec<Nullable<f64>>>,
    through: &JsDateOrString,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Nullable<ApiAirQualityHourlyContext>, CurrentProjectionError> {
    // map explicit null to absent context
    let values = match values {
        Nullable::Null => None,
        Nullable::Value(values) => Some(values),
    };
    project_air_quality_values(values, through, time_resolver)
}

/// project one particulate window
fn project_air_quality_values(
    values: Option<&Vec<Nullable<f64>>>,
    through: &JsDateOrString,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Nullable<ApiAirQualityHourlyContext>, CurrentProjectionError> {
    let Some(values) = values else {
        return Ok(Nullable::Null);
    };

    // require the complete newest-first nowcast window
    if values.len() != 12
        || values.iter().any(|value| {
            matches!(value, Nullable::Value(number) if !number.is_finite() || *number < 0.0)
        })
    {
        return Ok(Nullable::Null);
    }

    Ok(Nullable::Value(ApiAirQualityHourlyContext {
        hourlyPm25: values.clone(),
        through: time_resolver.resolve(through)?.utc,
    }))
}

/// copy canonical weather metrics
fn project_metrics(row: &CurrentWeatherRecordRow) -> ApiWeatherRecordMetrics {
    ApiWeatherRecordMetrics {
        apparentTemperatureC: row.apparentTemperatureC.clone(),
        blackGlobeTemperatureC: row.blackGlobeTemperatureC.clone(),
        cloudCoverPercent: row.cloudCoverPercent.clone(),
        pm25MicrogramsPerCubicMeter: row.pm25MicrogramsPerCubicMeter.clone(),
        precipitationMm: row.precipitationMm.clone(),
        precipitationRateMmPerHour: row.precipitationRateMmPerHour.clone(),
        pressureHpa: row.pressureHpa.clone(),
        relativeHumidityPercent: row.relativeHumidityPercent.clone(),
        soilElectricalConductivityMicrosiemensPerCm: row
            .soilElectricalConductivityMicrosiemensPerCm
            .clone(),
        soilMoisturePercent: row.soilMoisturePercent.clone(),
        solarRadiationWm2: row.solarRadiationWm2.clone(),
        temperatureC: row.temperatureC.clone(),
        uvIndex: row.uvIndex.clone(),
        windDirectionDegrees: row.windDirectionDegrees.clone(),
        windGustMps: row.windGustMps.clone(),
        windSpeedMps: row.windSpeedMps.clone(),
        wetBulbGlobeTemperatureC: row.wetBulbGlobeTemperatureC.clone(),
    }
}

/// describe record recency
pub(crate) fn describe_freshness(
    valid_at_epoch_ms: i64,
    data_at_epoch_ms: i64,
) -> ApiWeatherRecordFreshness {
    let age_seconds = data_at_epoch_ms.saturating_sub(valid_at_epoch_ms).max(0) / 1_000;

    // mark values within two source cadences as fresh
    if age_seconds <= 1_800 {
        return ApiWeatherRecordFreshness {
            ageSeconds: age_seconds as u64,
            label: "Weather value is current".to_owned(),
            status: ApiWeatherRecordFreshnessStatusEnum::Fresh,
        };
    }

    // call out a moderate delay explicitly
    if age_seconds <= 7_200 {
        return ApiWeatherRecordFreshness {
            ageSeconds: age_seconds as u64,
            label: "Weather value is delayed".to_owned(),
            status: ApiWeatherRecordFreshnessStatusEnum::Delayed,
        };
    }

    ApiWeatherRecordFreshness {
        ageSeconds: age_seconds as u64,
        label: "Weather value may be stale".to_owned(),
        status: ApiWeatherRecordFreshnessStatusEnum::Stale,
    }
}

/// map one nullable database instant
pub(crate) fn map_nullable_instant(
    value: &Nullable<JsDateOrString>,
    time_resolver: &dyn CurrentTimeResolver,
) -> Result<Nullable<String>, CurrentProjectionError> {
    // preserve the explicit sql null
    match value {
        Nullable::Null => Ok(Nullable::Null),
        Nullable::Value(value) => Ok(Nullable::Value(time_resolver.resolve(value)?.utc)),
    }
}

/// find one ordered object key
pub(crate) fn object_value<'a>(
    entries: &'a [(Vec<u16>, JsonValue)],
    key: &str,
) -> Option<&'a JsonValue> {
    entries.iter().find_map(|(candidate, value)| {
        candidate
            .iter()
            .copied()
            .eq(key.encode_utf16())
            .then_some(value)
    })
}

/// project one bounded string
fn bounded_string(
    value: Option<&JsonValue>,
    maximum_utf16: usize,
) -> Result<Nullable<String>, CurrentProjectionError> {
    let Some(JsonValue::String(value)) = value else {
        return Ok(Nullable::Null);
    };
    let end = value.len().min(maximum_utf16);
    Ok(Nullable::Value(utf16_string(&value[..end])?))
}

/// project one finite number
fn bounded_number(value: Option<&JsonValue>) -> Nullable<f64> {
    // retain only finite numeric metadata
    match value {
        Some(JsonValue::Number(value)) if value.is_finite() => Nullable::Value(*value),
        _ => Nullable::Null,
    }
}

/// project one bounded string list
fn bounded_strings(
    value: Option<&JsonValue>,
) -> Result<Nullable<Vec<String>>, CurrentProjectionError> {
    let Some(JsonValue::Array(values)) = value else {
        return Ok(Nullable::Null);
    };
    let mut result = Vec::new();

    // retain the first twenty string entries
    for value in values.iter().take(20) {
        let JsonValue::String(value) = value else {
            continue;
        };
        let end = value.len().min(128);
        result.push(utf16_string(&value[..end])?);
    }

    Ok(Nullable::Value(result))
}

/// decode generated storage text
fn utf16_string(value: &[u16]) -> Result<String, CurrentProjectionError> {
    String::from_utf16(value).map_err(|_| CurrentProjectionError {
        code: "current_projection.utf16",
        detail: "database metadata contains invalid UTF-16".to_owned(),
    })
}

/// validate one sensor metric key
fn is_metric_key(value: &str) -> bool {
    let mut chars = value.chars();

    // require one leading lowercase ascii character
    let Some(first) = chars.next() else {
        return false;
    };

    // enforce the bounded ascii grammar
    first.is_ascii_lowercase()
        && value.len() <= 64
        && chars.all(|character| character.is_ascii_alphanumeric())
}

/// map one source label
pub(crate) fn source_kind_label(source_kind: SourceKind) -> &'static str {
    // preserve the exhaustive typescript labels
    match source_kind {
        SourceKind::Forecast => "forecast model product",
        SourceKind::ModelCurrent => "model-derived current conditions",
        SourceKind::PhysicalSensor => "physical sensor measurement",
        SourceKind::Reanalysis => "gridded historical reanalysis",
        SourceKind::TideObservation => "observed tide level",
        SourceKind::TidePrediction => "tide prediction",
    }
}
