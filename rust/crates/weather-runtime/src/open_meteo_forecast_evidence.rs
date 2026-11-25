//! Build paired Open-Meteo forecast response evidence through admitted value owners.

use weather_contracts::JsonValue;
use weather_database::{SerializedJson, Sha256Digest};
use weather_worker::TaskFailure;

/// One parsed upstream response's already-admitted evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoForecastResponseEvidence {
    pub checksum: Sha256Digest,
    pub generation_ms: Option<f64>,
    pub http_status: u16,
}

/// Approved hashing and JSON boundaries for paired forecast evidence.
pub trait OpenMeteoForecastEvidenceValues {
    /// Hash the exact paired upstream checksum bytes.
    fn forecast_sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, TaskFailure>;

    /// Serialize the ordered forecast response metadata object.
    fn forecast_metadata_json(
        &mut self,
        entries: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure>;
}

/// Build Node-compatible paired forecast checksum and ordered response metadata.
pub fn build_open_meteo_forecast_evidence(
    weather: &OpenMeteoForecastResponseEvidence,
    air_quality: &OpenMeteoForecastResponseEvidence,
    values: &mut impl OpenMeteoForecastEvidenceValues,
) -> Result<(Sha256Digest, SerializedJson), TaskFailure> {
    let mut checksum_bytes = Vec::with_capacity(
        weather.checksum.as_str().len() + air_quality.checksum.as_str().len() + 2,
    );
    checksum_bytes.extend_from_slice(weather.checksum.as_str().as_bytes());
    checksum_bytes.push(b'\n');
    checksum_bytes.extend_from_slice(air_quality.checksum.as_str().as_bytes());
    checksum_bytes.push(b'\n');
    let checksum = values.forecast_sha256(&checksum_bytes)?;

    let mut metadata = Vec::with_capacity(5);
    metadata.push((
        metadata_key("air_quality_http_status"),
        JsonValue::Number(f64::from(air_quality.http_status)),
    ));
    // omit the optional air-quality generation exactly when Node rejects non-finite numbers
    if let Some(generation_ms) = air_quality.generation_ms.filter(|value| value.is_finite()) {
        metadata.push((
            metadata_key("air_quality_generation_ms"),
            JsonValue::Number(generation_ms),
        ));
    }
    metadata.push((
        metadata_key("http_status"),
        JsonValue::Number(f64::from(weather.http_status)),
    ));
    metadata.push((
        metadata_key("upstream_response_count"),
        JsonValue::Number(2.0),
    ));
    // append weather generation last to preserve the Node object construction order
    if let Some(generation_ms) = weather.generation_ms.filter(|value| value.is_finite()) {
        metadata.push((
            metadata_key("weather_generation_ms"),
            JsonValue::Number(generation_ms),
        ));
    }
    let response_metadata = values.forecast_metadata_json(&metadata)?;

    Ok((checksum, response_metadata))
}

/// Encode one metadata key using the generated JSON representation.
fn metadata_key(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEATHER_CHECKSUM: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const AIR_QUALITY_CHECKSUM: &str =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const COMBINED_CHECKSUM: &str =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    #[derive(Default)]
    struct FixtureValues {
        hash_inputs: Vec<Vec<u8>>,
        metadata_inputs: Vec<Vec<(Vec<u16>, JsonValue)>>,
    }

    impl OpenMeteoForecastEvidenceValues for FixtureValues {
        /// Retain the exact checksum material supplied to the approved owner.
        fn forecast_sha256(&mut self, bytes: &[u8]) -> Result<Sha256Digest, TaskFailure> {
            self.hash_inputs.push(bytes.to_vec());
            Ok(Sha256Digest::admitted(COMBINED_CHECKSUM))
        }

        /// Retain the exact ordered metadata fields supplied to the approved owner.
        fn forecast_metadata_json(
            &mut self,
            entries: &[(Vec<u16>, JsonValue)],
        ) -> Result<SerializedJson, TaskFailure> {
            self.metadata_inputs.push(entries.to_vec());
            Ok(SerializedJson::admitted(r#"{"fixture":true}"#))
        }
    }

    /// Preserve checksum boundaries and every finite metadata field in Node order.
    #[test]
    fn builds_paired_checksum_and_finite_metadata() {
        let weather = response(WEATHER_CHECKSUM, Some(12.5), 200);
        let air_quality = response(AIR_QUALITY_CHECKSUM, Some(7.25), 206);
        let mut values = FixtureValues::default();

        let result = build_open_meteo_forecast_evidence(&weather, &air_quality, &mut values)
            .expect("paired evidence should build");

        assert_eq!(result.0, Sha256Digest::admitted(COMBINED_CHECKSUM));
        assert_eq!(result.1, SerializedJson::admitted(r#"{"fixture":true}"#));
        assert_eq!(
            values.hash_inputs,
            [format!("{WEATHER_CHECKSUM}\n{AIR_QUALITY_CHECKSUM}\n").into_bytes()]
        );
        assert_eq!(
            values.metadata_inputs,
            [vec![
                field("air_quality_http_status", 206.0),
                field("air_quality_generation_ms", 7.25),
                field("http_status", 200.0),
                field("upstream_response_count", 2.0),
                field("weather_generation_ms", 12.5),
            ]]
        );
    }

    /// Omit non-finite generation values without changing required field order.
    #[test]
    fn omits_nonfinite_generation_metadata() {
        let weather = response(WEATHER_CHECKSUM, Some(f64::NAN), 502);
        let air_quality = response(AIR_QUALITY_CHECKSUM, Some(f64::INFINITY), 429);
        let mut values = FixtureValues::default();

        build_open_meteo_forecast_evidence(&weather, &air_quality, &mut values)
            .expect("required metadata should remain serializable");

        assert_eq!(
            values.metadata_inputs,
            [vec![
                field("air_quality_http_status", 429.0),
                field("http_status", 502.0),
                field("upstream_response_count", 2.0),
            ]]
        );
    }

    /// Build one typed response fixture.
    fn response(
        checksum: &str,
        generation_ms: Option<f64>,
        http_status: u16,
    ) -> OpenMeteoForecastResponseEvidence {
        OpenMeteoForecastResponseEvidence {
            checksum: Sha256Digest::admitted(checksum),
            generation_ms,
            http_status,
        }
    }

    /// Build one ordered numeric metadata field.
    fn field(key: &str, value: f64) -> (Vec<u16>, JsonValue) {
        (metadata_key(key), JsonValue::Number(value))
    }
}
