//! Decode PostgreSQL's actual typed result rows without going through intermediate JSON objects.

use serde_json::value::RawValue;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::{
    Row,
    types::{FromSqlOwned, Json, Type},
};
use weather_contracts::{
    CurrentWeatherRecordRow, JsDateOrString, JsonValue, Nullable, SourceKind, WeatherRecordRow,
};
use weather_database::{ActiveSiteDbRow, AdmittedRevisionUtcInstant, Sha256Digest};
use weather_edge::ArchiveWirePrimitives;

use crate::{NativePostgresError, NativeWirePrimitives, native_json::NativeJson};

/// Preserve post-query decode failure classification at the typed row boundary.
pub(crate) fn column<T: FromSqlOwned>(row: &Row, name: &str) -> Result<T, NativePostgresError> {
    row.try_get(name)
        .map_err(|error| NativePostgresError::decode(name, error))
}

/// Preserve explicit SQL nulls in the generated storage representation.
pub(crate) fn optional<T>(value: Option<T>) -> Nullable<T> {
    value.map_or(Nullable::Null, Nullable::Value)
}

/// Decode nullable scalar columns directly from the driver's binary representation.
pub(crate) fn nullable<T: FromSqlOwned>(
    row: &Row,
    name: &str,
) -> Result<Nullable<T>, NativePostgresError> {
    column::<Option<T>>(row, name).map(optional)
}

/// Preserve predecessor bigint string identity without a lossy JavaScript number conversion.
pub(crate) fn id(row: &Row, name: &str) -> Result<String, NativePostgresError> {
    // postgres casts explicit text identifiers in a few queries
    let column_type = row
        .columns()
        .iter()
        .find(|column| column.name() == name)
        .map(|column| column.type_());
    match column_type {
        Some(&Type::INT8) => column::<i64>(row, name).map(|value| value.to_string()),
        Some(&Type::UUID) => column::<uuid::Uuid>(row, name).map(|value| value.to_string()),
        _ => column(row, name),
    }
}

/// Convert the driver's integer representation to the existing unsigned storage domain.
pub(crate) fn unsigned(row: &Row, name: &str) -> Result<u64, NativePostgresError> {
    let column_type = row
        .columns()
        .iter()
        .find(|column| column.name() == name)
        .map(|column| column.type_());
    // counts may be postgres smallint, integer, bigint, or intentionally cast text
    let value = match column_type {
        Some(&Type::INT2) => i64::from(column::<i16>(row, name)?),
        Some(&Type::INT4) => i64::from(column::<i32>(row, name)?),
        Some(&Type::INT8) => column::<i64>(row, name)?,
        _ => {
            return column::<String>(row, name)?
                .parse::<u64>()
                .map_err(|error| NativePostgresError::decode(name, error));
        }
    };
    u64::try_from(value).map_err(|error| NativePostgresError::decode(name, error))
}

/// Convert native PostgreSQL time to the predecessor's millisecond Date value.
pub(crate) fn epoch_milliseconds(time: SystemTime) -> Option<i64> {
    // preserve pre-epoch milliseconds by flooring the fractional calendar second
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).ok(),
        Err(error) => {
            let nanos = i128::try_from(error.duration().as_nanos()).ok()?;
            i64::try_from((-nanos).div_euclid(1_000_000)).ok()
        }
    }
}

/// Decode a nullable or required driver timestamp as a generated Date.
pub(crate) fn date(row: &Row, name: &str) -> Result<JsDateOrString, NativePostgresError> {
    Ok(date_value(column::<SystemTime>(row, name)?))
}

/// Bind native time to the valid/invalid Date storage contract.
fn date_value(time: SystemTime) -> JsDateOrString {
    epoch_milliseconds(time)
        .filter(|value| (-8_640_000_000_000_000..=8_640_000_000_000_000).contains(value))
        .map_or(
            JsDateOrString::InvalidDate,
            JsDateOrString::DateEpochMilliseconds,
        )
}

/// Preserve nullable timestamp semantics for the weather product run.
fn nullable_date(row: &Row, name: &str) -> Result<Nullable<JsDateOrString>, NativePostgresError> {
    Ok(optional(
        column::<Option<SystemTime>>(row, name)?.map(date_value),
    ))
}

/// Decode a timestamp that must be canonical UTC for a retained typed record.
pub(crate) fn utc(row: &Row, name: &str) -> Result<String, NativePostgresError> {
    utc_value(column::<SystemTime>(row, name)?, name)
}

/// Admit the nullable canonical archive timestamp fields.
pub(crate) fn optional_utc(
    row: &Row,
    name: &str,
) -> Result<Option<AdmittedRevisionUtcInstant>, NativePostgresError> {
    column::<Option<SystemTime>>(row, name)?
        .map(|time| utc_value(time, name).map(AdmittedRevisionUtcInstant::admitted))
        .transpose()
}

/// Format the original millisecond UTC witness through the approved wire owner.
fn utc_value(time: SystemTime, name: &str) -> Result<String, NativePostgresError> {
    epoch_milliseconds(time)
        .and_then(|epoch| NativeWirePrimitives.format_utc_milliseconds(epoch))
        .ok_or_else(|| NativePostgresError::decode(name, "invalid timestamp"))
}

/// Read a JSON/JSONB column without sorting away the predecessor's stored object order.
pub(crate) fn json(row: &Row, name: &str) -> Result<Option<JsonValue>, NativePostgresError> {
    column::<Option<Json<Box<RawValue>>>>(row, name)?
        .map(|value| {
            NativeJson::decode(value.0.get().as_bytes())
                .ok_or_else(|| NativePostgresError::decode(name, "invalid JSON value"))
        })
        .transpose()
}

/// Ordered nullable provider or quality metadata from the actual database row.
type WeatherMetadata = Nullable<Vec<(Vec<u16>, JsonValue)>>;

/// Ordered nullable per-sensor concentration curves.
type SensorCurves = Nullable<Vec<(Vec<u16>, Vec<Nullable<f64>>)>>;

/// Decode a nullable provider/quality object using the existing ordered JSON owner.
fn object(row: &Row, name: &str) -> Result<WeatherMetadata, NativePostgresError> {
    match json(row, name)? {
        None | Some(JsonValue::Null) => Ok(Nullable::Null),
        Some(JsonValue::Object(fields)) => Ok(Nullable::Value(fields)),
        _ => Err(NativePostgresError::decode(name, "expected JSON object")),
    }
}

/// Admit an enum using the TypeScript-generated exact wire alternatives.
pub(crate) fn wire_enum<T: Copy>(
    row: &Row,
    name: &str,
    values: &[(&str, T)],
) -> Result<T, NativePostgresError> {
    let value = column::<String>(row, name)?;
    values
        .iter()
        .find_map(|(wire, variant)| (*wire == value).then_some(*variant))
        .ok_or_else(|| NativePostgresError::decode(name, "unrecognized enum"))
}

/// Admit server-owned lowercase digest witnesses through the shared digest grammar.
pub(crate) fn digest(row: &Row, name: &str) -> Result<Sha256Digest, NativePostgresError> {
    let value = column::<String>(row, name)?;
    crate::maintenance_canonical_json::valid_hash(&value)
        .then(|| Sha256Digest::admitted(value))
        .ok_or_else(|| NativePostgresError::decode(name, "invalid SHA-256"))
}

/// Read the ordered current-weather AQI curves from either JSONB or a native float array.
fn sensor_curves(row: &Row) -> Result<SensorCurves, NativePostgresError> {
    let name = "pm25HourlyBySensor";
    // null and an actual object remain distinct
    match json(row, name)? {
        None | Some(JsonValue::Null) => Ok(Nullable::Null),
        Some(JsonValue::Object(fields)) => fields
            .into_iter()
            .map(|(key, value)| {
                let JsonValue::Array(values) = value else {
                    return Err(NativePostgresError::decode(
                        name,
                        "expected concentration array",
                    ));
                };
                let values = values
                    .into_iter()
                    .map(|value| match value {
                        JsonValue::Null => Ok(Nullable::Null),
                        JsonValue::Number(number) => Ok(Nullable::Value(number)),
                        _ => Err(NativePostgresError::decode(
                            name,
                            "expected concentration number",
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((key, values))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Nullable::Value),
        _ => Err(NativePostgresError::decode(name, "expected sensor object")),
    }
}

// retain the identical common weather-column mapping for both generated row types
macro_rules! weather_row {
    ($kind:ident, $row:ident $(, $field:ident: $value:expr)*) => { $kind {
        adapterVersion: nullable($row, "adapterVersion")?,
        apparentTemperatureC: nullable($row, "apparentTemperatureC")?,
        blackGlobeTemperatureC: nullable($row, "blackGlobeTemperatureC")?,
        cloudCoverPercent: nullable($row, "cloudCoverPercent")?,
        contentHash: column($row, "contentHash")?,
        contractEpoch: nullable($row, "contractEpoch")?,
        deviceModel: nullable($row, "deviceModel")?,
        deviceSerial: nullable($row, "deviceSerial")?,
        deviceVendor: nullable($row, "deviceVendor")?,
        firstReceivedAt: date($row, "firstReceivedAt")?,
        id: id($row, "id")?,
        lastReceivedAt: date($row, "lastReceivedAt")?,
        pm25MicrogramsPerCubicMeter: nullable($row, "pm25MicrogramsPerCubicMeter")?,
        precipitationMm: nullable($row, "precipitationMm")?,
        precipitationRateMmPerHour: nullable($row, "precipitationRateMmPerHour")?,
        pressureHpa: nullable($row, "pressureHpa")?,
        productRunAt: nullable_date($row, "productRunAt")?,
        providerKey: column($row, "providerKey")?,
        providerMetadata: object($row, "providerMetadata")?,
        qualityMetadata: object($row, "qualityMetadata")?,
        relativeHumidityPercent: nullable($row, "relativeHumidityPercent")?,
        revisionCount: unsigned($row, "revisionCount")?,
        siteSlug: column($row, "siteSlug")?,
        sourceId: id($row, "sourceId")?,
        sourceConfigFingerprint: nullable($row, "sourceConfigFingerprint")?,
        sourceKey: column($row, "sourceKey")?,
        sourceKind: wire_enum($row, "sourceKind", SourceKind::WIRE_VALUES)?,
        stationSlug: column($row, "stationSlug")?,
        soilElectricalConductivityMicrosiemensPerCm: nullable($row, "soilElectricalConductivityMicrosiemensPerCm")?,
        soilMoisturePercent: nullable($row, "soilMoisturePercent")?,
        solarRadiationWm2: nullable($row, "solarRadiationWm2")?,
        temperatureC: nullable($row, "temperatureC")?,
        upstreamModel: nullable($row, "upstreamModel")?,
        upstreamTimezone: column($row, "upstreamTimezone")?,
        uvIndex: nullable($row, "uvIndex")?,
        validAt: date($row, "validAt")?,
        waterLevelM: nullable($row, "waterLevelM")?,
        windDirectionDegrees: nullable($row, "windDirectionDegrees")?,
        windGustMps: nullable($row, "windGustMps")?,
        windSpeedMps: nullable($row, "windSpeedMps")?,
        wetBulbGlobeTemperatureC: nullable($row, "wetBulbGlobeTemperatureC")?,
        $($field: $value,)*
    }};
}

/// Decode the exact history/forecast result aliases directly into generated storage.
pub(crate) fn weather(row: &Row) -> Result<WeatherRecordRow, NativePostgresError> {
    Ok(weather_row!(WeatherRecordRow, row))
}

/// Decode the current row and the independently projected current-only AQI fields.
pub(crate) fn current(row: &Row) -> Result<CurrentWeatherRecordRow, NativePostgresError> {
    let concentrations = column::<Option<Vec<Option<f64>>>>(row, "pm25HourlyConcentrations")?
        .map(|values| values.into_iter().map(optional).collect());
    Ok(weather_row!(CurrentWeatherRecordRow, row,
        pm25HourlyBySensor: sensor_curves(row)?,
        pm25HourlyConcentrations: optional(concentrations),
        pm25HourlyThrough: date(row, "pm25HourlyThrough")?,
        pressureChange3hHpa: nullable(row, "pressureChange3hHpa")?
    ))
}

/// Decode the active-site aliases in their original database order.
pub(crate) fn active_site(row: &Row) -> Result<ActiveSiteDbRow, NativePostgresError> {
    Ok(ActiveSiteDbRow {
        attribution_label: column(row, "attributionLabel")?,
        attribution_url: column(row, "attributionUrl")?,
        latitude: column(row, "latitude")?,
        longitude: column(row, "longitude")?,
        provider_key: column(row, "providerKey")?,
        provider_name: column(row, "providerName")?,
        site_name: column(row, "siteName")?,
        site_slug: column(row, "siteSlug")?,
        source_id: id(row, "sourceId")?,
        source_key: column(row, "sourceKey")?,
        source_kind: column(row, "sourceKind")?,
        station_kind: column(row, "stationKind")?,
        station_latitude: column(row, "stationLatitude")?,
        station_longitude: column(row, "stationLongitude")?,
        station_name: column(row, "stationName")?,
        station_slug: column(row, "stationSlug")?,
        timezone: column(row, "timezone")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Keep PostgreSQL sub-millisecond timestamp conversion in the correct Date slot.
    #[test]
    fn native_database_date_preserves_negative_epoch_geometry() {
        assert_eq!(
            epoch_milliseconds(UNIX_EPOCH + Duration::from_micros(1999)),
            Some(1)
        );
        assert_eq!(
            epoch_milliseconds(UNIX_EPOCH - Duration::from_micros(1999)),
            Some(-2)
        );
        assert_eq!(epoch_milliseconds(UNIX_EPOCH), Some(0));
    }
}
