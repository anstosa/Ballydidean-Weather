//! Borrowed JSON wire views for public and private Weather service replies.

use crate::service::{
    serve_decoded_request_representation, serve_decoded_request_representation_reporting,
    serve_private_decoded_request_representation,
};
use crate::{
    AdmittedPrivateRouteAuthority, DecodedServiceRequest,
    FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES, WeatherServiceBody, WeatherServiceBoundaries,
    WeatherServiceClock, WeatherServiceRepository, WeatherServiceResponse,
};

const APPLICATION_CACHE_CONTROL: &str = "no-store";
const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";
const READ_ALLOW: &str = "GET, HEAD";
const APPLICATION_INTERNAL_ERROR_JSON: &[u8] =
    b"{\"error\":{\"code\":\"internal_error\",\"message\":\"Unexpected server error\"}}";

/// Approved serializer whose returned bytes remain owned by the encoder.
pub trait WeatherServiceWireEncoder {
    type Error;

    /// Encode one typed service body exactly like `JSON.stringify`, without a trailing newline.
    fn encode<'a>(&'a mut self, body: &WeatherServiceBody) -> Result<&'a [u8], Self::Error>;
}

/// Headers selected by the application JSON response owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherServiceWireHeaders {
    pub cache_control: Option<&'static str>,
    pub content_type: &'static str,
    pub allow: Option<&'static str>,
}

/// Borrowed response representation; the transport owns framing and byte length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherServiceWireResponse<'a> {
    pub status: u16,
    pub headers: WeatherServiceWireHeaders,
    pub body: &'a [u8],
}

/// bounded rain fallback detail exposed only to an explicitly supplied reporter
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeatherServiceRainFallbackDiagnostic {
    pub error_code: &'static str,
    pub error_name: &'static str,
    pub event: &'static str,
    pub method: &'static str,
    pub operation: &'static str,
    pub status: u16,
}

/// Materialize one typed application reply or its bounded application encoding failure.
fn materialize_weather_service_response<'a, Encoder: WeatherServiceWireEncoder>(
    response: WeatherServiceResponse,
    omit_body: bool,
    encoder: &'a mut Encoder,
) -> WeatherServiceWireResponse<'a> {
    let headers = application_headers(response.allow_get_head);
    let Some(body) = response.body else {
        return WeatherServiceWireResponse {
            status: response.status,
            headers,
            body: &[],
        };
    };
    let bounded_private_observation =
        matches!(&body, WeatherServiceBody::ForecastObservationInput(_));
    let mut wire = match encoder.encode(&body) {
        // enforce the source's private envelope limit on the complete encoded body
        Ok(body)
            if bounded_private_observation
                && body.len() > FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES =>
        {
            application_internal_error()
        }
        Ok(body) => WeatherServiceWireResponse {
            status: response.status,
            headers,
            body,
        },
        Err(_) => application_internal_error(),
    };
    // the source completes serialization before applying head suppression
    if omit_body {
        wire.body = &[];
    }
    wire
}

/// Execute the existing public service and project its result into a borrowed wire response.
pub fn serve_wire_decoded_request<'a, Encoder: WeatherServiceWireEncoder>(
    repository: &mut dyn WeatherServiceRepository,
    clock: &mut dyn WeatherServiceClock,
    boundaries: &WeatherServiceBoundaries<'_>,
    request: DecodedServiceRequest<'_>,
    encoder: &'a mut Encoder,
) -> WeatherServiceWireResponse<'a> {
    let omit_body = request.method == "HEAD";
    let response = serve_decoded_request_representation(repository, clock, boundaries, request);
    materialize_weather_service_response(response, omit_body, encoder)
}

/// execute the public service and report one bounded rain fallback after final encoding
pub fn serve_wire_decoded_request_with_rain_diagnostics<'a, Encoder: WeatherServiceWireEncoder>(
    repository: &mut dyn WeatherServiceRepository,
    clock: &mut dyn WeatherServiceClock,
    boundaries: &WeatherServiceBoundaries<'_>,
    request: DecodedServiceRequest<'_>,
    encoder: &'a mut Encoder,
    reporter: &mut dyn FnMut(WeatherServiceRainFallbackDiagnostic) -> Result<(), ()>,
) -> WeatherServiceWireResponse<'a> {
    let omit_body = request.method == "HEAD";
    let mut rain_read_failed = false;
    let response = serve_decoded_request_representation_reporting(
        repository,
        clock,
        boundaries,
        request,
        Some(&mut rain_read_failed),
    );
    let wire = materialize_weather_service_response(response, omit_body, encoder);
    // report only after encoding fixes the final public status
    if rain_read_failed {
        let _ = reporter(WeatherServiceRainFallbackDiagnostic {
            error_code: "rain_sidecar_read_failed",
            error_name: "UnknownError",
            event: "rain_adjustment_fallback",
            method: rain_fallback_diagnostic_method(request.method),
            operation: "sidecar_read",
            status: wire.status,
        });
    }
    wire
}

/// Execute the admitted private observation service and project its borrowed wire response.
pub fn serve_private_wire_decoded_request<'a, Encoder: WeatherServiceWireEncoder>(
    authority: AdmittedPrivateRouteAuthority,
    repository: &mut dyn WeatherServiceRepository,
    clock: &mut dyn WeatherServiceClock,
    boundaries: &WeatherServiceBoundaries<'_>,
    request: DecodedServiceRequest<'_>,
    encoder: &'a mut Encoder,
) -> WeatherServiceWireResponse<'a> {
    let omit_body = request.method == "HEAD";
    let response = serve_private_decoded_request_representation(
        authority, repository, clock, boundaries, request,
    );
    materialize_weather_service_response(response, omit_body, encoder)
}

/// Select the application JSON headers while retaining the optional read-only advertisement.
const fn application_headers(allow_get_head: bool) -> WeatherServiceWireHeaders {
    let allow = match allow_get_head {
        true => Some(READ_ALLOW),
        false => None,
    };
    WeatherServiceWireHeaders {
        cache_control: Some(APPLICATION_CACHE_CONTROL),
        content_type: JSON_CONTENT_TYPE,
        allow,
    }
}

/// Preserve `errorResponse` policy when application JSON serialization cannot complete.
const fn application_internal_error() -> WeatherServiceWireResponse<'static> {
    WeatherServiceWireResponse {
        status: 500,
        headers: WeatherServiceWireHeaders {
            cache_control: Some(APPLICATION_CACHE_CONTROL),
            content_type: JSON_CONTENT_TYPE,
            allow: None,
        },
        body: APPLICATION_INTERNAL_ERROR_JSON,
    }
}

/// bound diagnostic methods to the source-compatible public method vocabulary
fn rain_fallback_diagnostic_method(method: &str) -> &'static str {
    match method {
        "GET" => "GET",
        "HEAD" => "HEAD",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use weather_contracts::{ApiErrorResponse, ApiErrorResponseError};

    use super::*;
    use crate::{
        ForecastObservationInputResponse, ForecastObservationInputResponseContractVersionLiteral,
        ForecastObservationInputResponseSite,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FakeEncodeError;

    struct FakeEncoder {
        bytes: Vec<u8>,
        calls: usize,
        fail: bool,
    }

    impl FakeEncoder {
        /// Construct one fixture encoder with an explicit returned representation.
        fn returning(bytes: &[u8]) -> Self {
            Self {
                bytes: bytes.to_vec(),
                calls: 0,
                fail: false,
            }
        }

        /// Construct one fixture encoder that models a serializer failure.
        fn failing() -> Self {
            Self {
                bytes: Vec::new(),
                calls: 0,
                fail: true,
            }
        }
    }

    impl WeatherServiceWireEncoder for FakeEncoder {
        type Error = FakeEncodeError;

        /// Return fixture-owned bytes or the selected ordinary encoding failure.
        fn encode<'a>(&'a mut self, _body: &WeatherServiceBody) -> Result<&'a [u8], Self::Error> {
            self.calls += 1;
            // Preserve the selected fixture outcome after recording the call.
            if self.fail {
                Err(FakeEncodeError)
            } else {
                Ok(&self.bytes)
            }
        }
    }

    /// Construct one typed application body without serializing it in this module.
    fn error_body() -> WeatherServiceBody {
        WeatherServiceBody::Error(ApiErrorResponse {
            error: ApiErrorResponseError {
                code: "not_found".to_owned(),
                message: "Endpoint not found".to_owned(),
            },
        })
    }

    /// Construct the private body kind whose encoded representation has a fixed ceiling.
    fn forecast_observation_body() -> WeatherServiceBody {
        WeatherServiceBody::ForecastObservationInput(Box::new(ForecastObservationInputResponse {
            contractVersion: ForecastObservationInputResponseContractVersionLiteral,
            generatedAt: String::new(),
            projectionFrom: String::new(),
            queryFrom: String::new(),
            records: Vec::new(),
            site: ForecastObservationInputResponseSite {
                slug: "ballydidean".to_owned(),
                timezone: "America/Los_Angeles".to_owned(),
            },
            through: String::new(),
        }))
    }

    /// Prove an ordinary encoded application reply retains no-store and no trailing newline.
    #[test]
    fn materializes_application_json_view() {
        let json = b"{\"error\":{\"code\":\"not_found\",\"message\":\"Endpoint not found\"}}";
        let mut encoder = FakeEncoder::returning(json);

        let response = materialize_weather_service_response(
            WeatherServiceResponse {
                status: 404,
                allow_get_head: false,
                body: Some(error_body()),
            },
            false,
            &mut encoder,
        );

        assert_eq!(response.status, 404);
        assert_eq!(response.headers.cache_control, Some("no-store"));
        assert_eq!(response.headers.content_type, JSON_CONTENT_TYPE);
        assert_eq!(response.headers.allow, None);
        assert_eq!(response.body, json);
        assert!(!response.body.ends_with(b"\n"));
        assert_eq!(encoder.calls, 1);
    }

    /// Prove successful head suppression happens only after representation encoding.
    #[test]
    fn suppresses_successful_head_after_encoding() {
        let mut encoder = FakeEncoder::returning(b"{\"status\":\"fixture\"}");

        let response = materialize_weather_service_response(
            WeatherServiceResponse {
                status: 405,
                allow_get_head: true,
                body: Some(error_body()),
            },
            true,
            &mut encoder,
        );

        assert_eq!(response.status, 405);
        assert_eq!(response.headers.cache_control, Some("no-store"));
        assert_eq!(response.headers.allow, Some("GET, HEAD"));
        assert!(response.body.is_empty());
        assert_eq!(encoder.calls, 1);
    }

    /// Prove serializer failure uses the application error response and no-store policy.
    #[test]
    fn materializes_application_encoding_failure() {
        let mut encoder = FakeEncoder::failing();

        let response = materialize_weather_service_response(
            WeatherServiceResponse {
                status: 200,
                allow_get_head: false,
                body: Some(error_body()),
            },
            false,
            &mut encoder,
        );

        assert_eq!(response.status, 500);
        assert_eq!(response.headers.cache_control, Some("no-store"));
        assert_eq!(response.headers.content_type, JSON_CONTENT_TYPE);
        assert_eq!(response.headers.allow, None);
        assert_eq!(response.body, APPLICATION_INTERNAL_ERROR_JSON);
        assert!(!response.body.ends_with(b"\n"));
        assert_eq!(encoder.calls, 1);
    }

    /// Admit an exact-limit private body for GET and encode it once before returning.
    #[test]
    fn admits_private_observation_at_exact_wire_limit() {
        let bytes = vec![b'x'; FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES];
        let mut encoder = FakeEncoder::returning(&bytes);

        let response = materialize_weather_service_response(
            WeatherServiceResponse {
                status: 200,
                allow_get_head: false,
                body: Some(forecast_observation_body()),
            },
            false,
            &mut encoder,
        );

        assert_eq!(response.status, 200);
        assert_eq!(
            response.body.len(),
            FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES
        );
        assert_eq!(encoder.calls, 1);
    }

    /// Refuse one byte beyond the private limit before successful GET delivery.
    #[test]
    fn refuses_oversized_private_observation_get() {
        let bytes = vec![b'x'; FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES + 1];
        let mut encoder = FakeEncoder::returning(&bytes);

        let response = materialize_weather_service_response(
            WeatherServiceResponse {
                status: 200,
                allow_get_head: false,
                body: Some(forecast_observation_body()),
            },
            false,
            &mut encoder,
        );

        assert_eq!(response.status, 500);
        assert_eq!(response.body, APPLICATION_INTERNAL_ERROR_JSON);
        assert_eq!(encoder.calls, 1);
    }

    /// Encode and check the private limit before suppressing either successful or failed HEAD bodies.
    #[test]
    fn checks_private_observation_limit_before_head_suppression() {
        let cases = [
            (FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES, 200),
            (FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES + 1, 500),
        ];

        // exercise both sides of the exact byte boundary independently
        for (length, expected_status) in cases {
            let bytes = vec![b'x'; length];
            let mut encoder = FakeEncoder::returning(&bytes);
            let response = materialize_weather_service_response(
                WeatherServiceResponse {
                    status: 200,
                    allow_get_head: false,
                    body: Some(forecast_observation_body()),
                },
                true,
                &mut encoder,
            );

            assert_eq!(response.status, expected_status);
            assert!(response.body.is_empty());
            assert_eq!(encoder.calls, 1);
        }
    }

    /// Keep other application responses outside the private observation limit.
    #[test]
    fn leaves_other_application_bodies_uncapped() {
        let bytes = vec![b'x'; FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES + 1];
        let mut encoder = FakeEncoder::returning(&bytes);

        let response = materialize_weather_service_response(
            WeatherServiceResponse {
                status: 404,
                allow_get_head: false,
                body: Some(error_body()),
            },
            false,
            &mut encoder,
        );

        assert_eq!(response.status, 404);
        assert_eq!(
            response.body.len(),
            FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES + 1
        );
        assert_eq!(encoder.calls, 1);
    }

    /// keep diagnostic method values bounded to GET, HEAD, or UNKNOWN
    #[test]
    fn bounds_rain_fallback_diagnostic_methods() {
        assert_eq!(rain_fallback_diagnostic_method("GET"), "GET");
        assert_eq!(rain_fallback_diagnostic_method("HEAD"), "HEAD");
        assert_eq!(rain_fallback_diagnostic_method("POST"), "UNKNOWN");
    }
}
