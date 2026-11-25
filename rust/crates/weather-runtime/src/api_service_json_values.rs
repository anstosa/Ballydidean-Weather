//! Concrete API response values using the generated TypeScript contract and shared serializer.

use weather_api::{WeatherServiceBody, WeatherServiceWireEncoder};
use weather_contracts::{JsonValue, Nullable};
use weather_edge::ArchiveWirePrimitives;

use crate::api_service_forecast_values::{forecast_value, health_value};
use crate::api_service_other_values::{
    observation_value, precipitation_value, rain_collection_value, tides_value, trends_value,
};
use crate::api_service_weather_values::{current_value, history_value, sites_value};

/// Keep the selected serializer and only the most recent complete response bytes.
pub struct NativeWeatherServiceWireEncoder<P> {
    primitives: P,
    bytes: Vec<u8>,
}

impl<P: ArchiveWirePrimitives> NativeWeatherServiceWireEncoder<P> {
    /// Bind an approved JSON primitive without creating another runtime dependency.
    pub const fn new(primitives: P) -> Self {
        Self {
            primitives,
            bytes: Vec::new(),
        }
    }
}

impl<P: ArchiveWirePrimitives> WeatherServiceWireEncoder for NativeWeatherServiceWireEncoder<P> {
    type Error = ();

    /// Materialize the existing service DTO in producer order with no final line feed.
    fn encode<'a>(&'a mut self, body: &WeatherServiceBody) -> Result<&'a [u8], Self::Error> {
        let value = weather_service_body_value(body);
        let mut bytes = self.primitives.encode_ordered_json(&value).ok_or(())?;
        // reuse the serializer buffer instead of copying it to remove the archive LF
        if bytes.pop() != Some(b'\n') {
            return Err(());
        }
        self.bytes = bytes;
        Ok(&self.bytes)
    }
}

/// Project every typed API success or error into its exact Node JSON member shape.
pub fn weather_service_body_value(body: &WeatherServiceBody) -> JsonValue {
    match body {
        WeatherServiceBody::Sites(value) => sites_value(value),
        WeatherServiceBody::Current(value) => current_value(value),
        WeatherServiceBody::History(value) => history_value(value),
        WeatherServiceBody::Tides(value) => tides_value(value),
        WeatherServiceBody::Forecast(value) => forecast_value(value),
        WeatherServiceBody::Trends(value) => trends_value(value),
        WeatherServiceBody::DailyPrecipitation(value) => precipitation_value(value),
        WeatherServiceBody::RainCollection(value) => rain_collection_value(value),
        WeatherServiceBody::ForecastObservationInput(value) => observation_value(value),
        WeatherServiceBody::Health(value) => health_value(value),
        WeatherServiceBody::Error(value) => object([(
            "error",
            object([
                ("code", text(&value.error.code)),
                ("message", text(&value.error.message)),
            ]),
        )]),
    }
}

/// Build a JSON object in the original producer's member order.
pub(crate) fn object<'a>(fields: impl IntoIterator<Item = (&'a str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| member(key, value))
            .collect(),
    )
}

/// Encode one field name without normalizing its UTF-16 units.
pub(crate) fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Retain one exact response string for the selected JSON primitive.
pub(crate) fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Represent one original boolean response field.
pub(crate) fn boolean(value: bool) -> JsonValue {
    JsonValue::Boolean(value)
}

/// Preserve finite JavaScript number values and JSON.stringify's nonfinite null behavior.
pub(crate) fn number(value: f64) -> JsonValue {
    if value.is_finite() {
        JsonValue::Number(value)
    } else {
        JsonValue::Null
    }
}

/// Represent one generated safe unsigned JavaScript number field.
pub(crate) fn unsigned(value: u64) -> JsonValue {
    number(value as f64)
}

/// Retain explicit JSON null separately from a present typed contract value.
pub(crate) fn nullable<T>(value: &Nullable<T>, project: impl FnOnce(&T) -> JsonValue) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => project(value),
    }
}

/// Preserve one typed response array without a second intermediate collection.
pub(crate) fn array<T>(values: &[T], project: impl FnMut(&T) -> JsonValue) -> JsonValue {
    JsonValue::Array(values.iter().map(project).collect())
}

/// Select the exact original spelling from the generated closed enumeration table.
pub(crate) fn enum_text<T: Copy + Eq>(value: T, wire: &[(&str, T)]) -> JsonValue {
    text(
        wire.iter()
            .find_map(|(name, candidate)| (*candidate == value).then_some(*name))
            .expect("generated enum value has a wire spelling"),
    )
}
