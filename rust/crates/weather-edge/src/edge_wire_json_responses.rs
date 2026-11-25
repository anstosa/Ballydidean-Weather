//! Ordinary JSON and text response materialization for typed edge outcomes.

use crate::edge_wire_response::EdgeWireResponse;
use crate::{
    ForecastAdjustmentSettingsEnvelope, ForecastAdjustmentSettingsOutcome,
    PropertySensorLayoutEntry, PropertySensorLayoutOutcome, WeatherViewerContextOutcome,
};

const INTERNAL_SERVER_ERROR_BODY: &[u8] = b"internal server error\n";
const INVALID_REQUEST_BODY: &[u8] = b"invalid request\n";
const METHOD_NOT_ALLOWED_BODY: &[u8] = b"method not allowed\n";

/// Approved typed JSON serializer for the three ordinary edge response families.
pub trait EdgeWireJsonEncoder {
    type Error;

    /// Encode `{data, error?}` with the trailing newline used by `sendJson`.
    fn encode_adjustment_settings(
        &mut self,
        envelope: &ForecastAdjustmentSettingsEnvelope,
    ) -> Result<Vec<u8>, Self::Error>;

    /// Encode the public `{data: [...]}` layout with the `sendJson` trailing newline.
    fn encode_property_sensor_layout(
        &mut self,
        entries: &[PropertySensorLayoutEntry],
    ) -> Result<Vec<u8>, Self::Error>;

    /// Encode the update `{data: entry}` layout with the `sendJson` trailing newline.
    fn encode_property_sensor_layout_entry(
        &mut self,
        entry: &PropertySensorLayoutEntry,
    ) -> Result<Vec<u8>, Self::Error>;

    /// Encode the viewer `{data:{homeNetwork}}` representation without a trailing newline.
    fn encode_viewer_context(&mut self, home_network: bool) -> Result<Vec<u8>, Self::Error>;
}

/// Materialize one adjustment-settings outcome with its exact JSON or text representation.
pub fn materialize_adjustment_settings<Encoder: EdgeWireJsonEncoder>(
    outcome: ForecastAdjustmentSettingsOutcome,
    encoder: &mut Encoder,
) -> Result<EdgeWireResponse, Encoder::Error> {
    let status_code = outcome.status_code();
    let cache_control = outcome.cache_control();
    match outcome {
        ForecastAdjustmentSettingsOutcome::Success {
            envelope,
            omit_body,
        } => Ok(EdgeWireResponse::json(
            status_code,
            encoder.encode_adjustment_settings(&envelope)?,
            omit_body,
            cache_control,
        )),
        ForecastAdjustmentSettingsOutcome::InvalidRequest => Ok(EdgeWireResponse::text(
            status_code,
            INVALID_REQUEST_BODY,
            cache_control,
            None,
        )),
        ForecastAdjustmentSettingsOutcome::StoreFailure => Ok(EdgeWireResponse::text(
            status_code,
            INTERNAL_SERVER_ERROR_BODY,
            cache_control,
            None,
        )),
        ForecastAdjustmentSettingsOutcome::MethodNotAllowed { allow } => {
            Ok(EdgeWireResponse::text(
                status_code,
                METHOD_NOT_ALLOWED_BODY,
                cache_control,
                Some(allow.header_value()),
            ))
        }
    }
}

/// Materialize one property-layout outcome with public-read or administrator-update policy.
pub fn materialize_property_sensor_layout<Encoder: EdgeWireJsonEncoder>(
    outcome: PropertySensorLayoutOutcome,
    encoder: &mut Encoder,
) -> Result<EdgeWireResponse, Encoder::Error> {
    let status_code = outcome.status_code();
    let cache_control = outcome.cache_control();
    match outcome {
        PropertySensorLayoutOutcome::ReadSuccess { data, omit_body } => Ok(EdgeWireResponse::json(
            status_code,
            encoder.encode_property_sensor_layout(&data)?,
            omit_body,
            cache_control,
        )),
        PropertySensorLayoutOutcome::UpdateSuccess { data } => Ok(EdgeWireResponse::json(
            status_code,
            encoder.encode_property_sensor_layout_entry(&data)?,
            false,
            cache_control,
        )),
        PropertySensorLayoutOutcome::InvalidRequest => Ok(EdgeWireResponse::text(
            status_code,
            INVALID_REQUEST_BODY,
            cache_control,
            None,
        )),
        PropertySensorLayoutOutcome::StoreFailure => Ok(EdgeWireResponse::text(
            status_code,
            INTERNAL_SERVER_ERROR_BODY,
            cache_control,
            None,
        )),
        PropertySensorLayoutOutcome::MethodNotAllowed { allow } => Ok(EdgeWireResponse::text(
            status_code,
            METHOD_NOT_ALLOWED_BODY,
            cache_control,
            Some(allow.header_value()),
        )),
    }
}

/// Materialize the viewer-context representation or its read-only method response.
pub fn materialize_viewer_context<Encoder: EdgeWireJsonEncoder>(
    outcome: WeatherViewerContextOutcome,
    encoder: &mut Encoder,
) -> Result<EdgeWireResponse, Encoder::Error> {
    let status_code = outcome.status_code();
    let cache_control = outcome.cache_control();
    match outcome {
        WeatherViewerContextOutcome::Success {
            home_network,
            omit_body,
        } => Ok(EdgeWireResponse::json(
            status_code,
            encoder.encode_viewer_context(home_network)?,
            omit_body,
            cache_control,
        )),
        WeatherViewerContextOutcome::MethodNotAllowed => Ok(EdgeWireResponse::text(
            status_code,
            METHOD_NOT_ALLOWED_BODY,
            cache_control,
            outcome.allow(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use crate::{
        AdmittedPropertySensorUpdateInstant, ForecastAdjustmentSettings,
        ForecastAdjustmentSettingsAllow, ForecastAdjustmentSettingsDiagnostic, PropertySensorIcon,
        PropertySensorLayoutAllow,
    };

    use super::*;

    const SETTINGS_JSON: &[u8] =
        b"{\"data\":{\"version\":1,\"temperature\":true,\"wind\":false,\"rain\":true}}\n";
    const LAYOUT_JSON: &[u8] = b"{\"data\":[]}\n";
    const LAYOUT_ENTRY_JSON: &[u8] = b"{\"data\":{\"sensorKey\":\"station\"}}\n";
    const VIEWER_JSON: &[u8] = b"{\"data\":{\"homeNetwork\":true}}";

    #[derive(Default)]
    struct FakeEncoder {
        settings_calls: usize,
        layout_counts: Vec<usize>,
        entry_keys: Vec<String>,
        viewer_values: Vec<bool>,
    }

    impl EdgeWireJsonEncoder for FakeEncoder {
        type Error = Infallible;

        /// Retain the typed settings call and return approved fixture bytes.
        fn encode_adjustment_settings(
            &mut self,
            _envelope: &ForecastAdjustmentSettingsEnvelope,
        ) -> Result<Vec<u8>, Self::Error> {
            self.settings_calls += 1;
            Ok(SETTINGS_JSON.to_vec())
        }

        /// Retain the ordered public layout length and return approved fixture bytes.
        fn encode_property_sensor_layout(
            &mut self,
            entries: &[PropertySensorLayoutEntry],
        ) -> Result<Vec<u8>, Self::Error> {
            self.layout_counts.push(entries.len());
            Ok(LAYOUT_JSON.to_vec())
        }

        /// Retain the updated sensor identity and return approved fixture bytes.
        fn encode_property_sensor_layout_entry(
            &mut self,
            entry: &PropertySensorLayoutEntry,
        ) -> Result<Vec<u8>, Self::Error> {
            self.entry_keys.push(entry.sensor_key.clone());
            Ok(LAYOUT_ENTRY_JSON.to_vec())
        }

        /// Retain the admitted viewer decision and return the no-newline fixture bytes.
        fn encode_viewer_context(&mut self, home_network: bool) -> Result<Vec<u8>, Self::Error> {
            self.viewer_values.push(home_network);
            Ok(VIEWER_JSON.to_vec())
        }
    }

    /// Construct one typed layout entry for public-read and update response fixtures.
    fn layout_entry() -> PropertySensorLayoutEntry {
        PropertySensorLayoutEntry {
            display_name: "Station".to_owned(),
            icon: Some(PropertySensorIcon::Temperature),
            latitude: 47.9,
            longitude: -122.4,
            sensor_key: "station".to_owned(),
            updated_at: AdmittedPropertySensorUpdateInstant::admitted("2026-11-23T12:00:00.000Z"),
        }
    }

    /// Prove settings HEAD retains the encoded representation length while omitting bytes.
    #[test]
    fn materializes_adjustment_settings_head_and_method_response() {
        let mut encoder = FakeEncoder::default();
        let response = materialize_adjustment_settings(
            ForecastAdjustmentSettingsOutcome::Success {
                envelope: ForecastAdjustmentSettingsEnvelope {
                    data: ForecastAdjustmentSettings {
                        version: 1,
                        temperature: true,
                        wind: false,
                        rain: true,
                    },
                    error: Some(ForecastAdjustmentSettingsDiagnostic::Unavailable),
                },
                omit_body: true,
            },
            &mut encoder,
        )
        .expect("settings response");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.headers.content_length, SETTINGS_JSON.len());
        assert_eq!(response.headers.cache_control, "no-store");
        assert_eq!(
            response.headers.content_type.as_deref(),
            Some("application/json; charset=utf-8")
        );
        assert_eq!(response.headers.allow, None);
        assert!(response.body.is_empty());
        assert_eq!(encoder.settings_calls, 1);

        let response = materialize_adjustment_settings(
            ForecastAdjustmentSettingsOutcome::MethodNotAllowed {
                allow: ForecastAdjustmentSettingsAllow::AdminReadWrite,
            },
            &mut encoder,
        )
        .expect("method response");
        assert_eq!(response.status_code, 405);
        assert_eq!(response.headers.allow, Some("GET, HEAD, PUT"));
        assert_eq!(response.body.as_bytes(), METHOD_NOT_ALLOWED_BODY);
        assert_eq!(encoder.settings_calls, 1);
    }

    /// Prove public layout reads and administrator updates retain separate cache policy.
    #[test]
    fn materializes_property_layout_read_and_update() {
        let mut encoder = FakeEncoder::default();
        let read = materialize_property_sensor_layout(
            PropertySensorLayoutOutcome::ReadSuccess {
                data: Vec::new(),
                omit_body: false,
            },
            &mut encoder,
        )
        .expect("layout read");
        assert_eq!(read.status_code, 200);
        assert_eq!(read.headers.cache_control, "no-cache");
        assert_eq!(read.body.as_bytes(), LAYOUT_JSON);

        let update = materialize_property_sensor_layout(
            PropertySensorLayoutOutcome::UpdateSuccess {
                data: layout_entry(),
            },
            &mut encoder,
        )
        .expect("layout update");
        assert_eq!(update.status_code, 200);
        assert_eq!(update.headers.cache_control, "no-store");
        assert_eq!(update.body.as_bytes(), LAYOUT_ENTRY_JSON);
        assert_eq!(encoder.layout_counts, [0]);
        assert_eq!(encoder.entry_keys, ["station"]);
    }

    /// Prove layout method rejection retains its route-specific Allow value.
    #[test]
    fn materializes_property_layout_method_response() {
        let mut encoder = FakeEncoder::default();
        let response = materialize_property_sensor_layout(
            PropertySensorLayoutOutcome::MethodNotAllowed {
                allow: PropertySensorLayoutAllow::AdminUpdate,
            },
            &mut encoder,
        )
        .expect("layout method response");

        assert_eq!(response.status_code, 405);
        assert_eq!(response.headers.allow, Some("PUT"));
        assert_eq!(response.body.as_bytes(), METHOD_NOT_ALLOWED_BODY);
    }

    /// Prove viewer HEAD uses the real no-newline representation length and private cache policy.
    #[test]
    fn materializes_viewer_head_and_method_response() {
        let mut encoder = FakeEncoder::default();
        let response = materialize_viewer_context(
            WeatherViewerContextOutcome::Success {
                home_network: true,
                omit_body: true,
            },
            &mut encoder,
        )
        .expect("viewer response");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.headers.content_length, VIEWER_JSON.len());
        assert_eq!(response.headers.cache_control, "private, no-store");
        assert!(response.body.is_empty());
        assert_eq!(encoder.viewer_values, [true]);

        let response =
            materialize_viewer_context(WeatherViewerContextOutcome::MethodNotAllowed, &mut encoder)
                .expect("viewer method response");
        assert_eq!(response.status_code, 405);
        assert_eq!(response.headers.allow, Some("GET, HEAD"));
        assert_eq!(response.body.as_bytes(), METHOD_NOT_ALLOWED_BODY);
        assert_eq!(encoder.viewer_values, [true]);
    }
}
