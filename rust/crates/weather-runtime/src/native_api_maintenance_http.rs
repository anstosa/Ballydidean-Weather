//! Actual private API maintenance ingress over the selected Hyper transport.

use std::time::Duration;

use hyper::{
    Request, Response,
    body::Incoming,
    http::{self, request::Parts},
};
use weather_api::{
    AdjustmentMaintenanceErrorCode, AdjustmentMaintenanceRequestClass,
    AdjustmentMaintenanceResponse, AdjustmentMaintenanceResponseBody,
    AdjustmentMaintenanceWireEncoder, DecodedAdjustmentMaintenanceRequest,
    MAX_ADJUSTMENT_MAINTENANCE_REQUEST_BYTES, classify_adjustment_maintenance_request,
    materialize_adjustment_maintenance_wire_response,
};

use crate::{
    ApiMaintenanceRequestBodyCodec, ApiMaintenanceRevisionEvidenceValues,
    ApiMaintenanceShadowEvidenceValues, EncodedApiMaintenanceRequestOwners, NativeHttpBody,
    PreparedPrivateMaintenanceApi, WeatherApiMaintenancePool, WeatherApiSharedPool,
    native_maintenance_http_response, read_native_http_body,
};

/// One admitted raw request or its fixed response selected before application dispatch.
enum NativeApiMaintenanceBody {
    Application(Vec<u8>),
    Fixed(AdjustmentMaintenanceResponse),
}

/// Retain request metadata separately from bounded bytes collected before pool acquisition.
pub struct NativeApiMaintenanceHttpRequest {
    parts: Parts,
    body: NativeApiMaintenanceBody,
}

impl NativeApiMaintenanceHttpRequest {
    /// Reject unmatched raw targets before polling their body, then collect admitted POST bytes.
    pub async fn read(request: Request<Incoming>, body_deadline: Duration) -> Self {
        let (parts, incoming) = request.into_parts();
        let decoded = DecodedAdjustmentMaintenanceRequest {
            method: parts.method.as_str(),
            pathname: origin_form_target(&parts),
        };
        let body = match classify_adjustment_maintenance_request(decoded) {
            AdjustmentMaintenanceRequestClass::Schedule => {
                // the source serves schedule reads without consuming an incoming representation
                NativeApiMaintenanceBody::Application(Vec::new())
            }
            AdjustmentMaintenanceRequestClass::Post => {
                match read_native_http_body(
                    incoming,
                    MAX_ADJUSTMENT_MAINTENANCE_REQUEST_BYTES,
                    body_deadline,
                )
                .await
                {
                    Ok(bytes) => NativeApiMaintenanceBody::Application(bytes),
                    // collapse body transport, deadline, and capacity failures to the source response
                    Err(_) => NativeApiMaintenanceBody::Fixed(shadow_rejected()),
                }
            }
            AdjustmentMaintenanceRequestClass::NotFound => {
                // retain exact method and raw-target rejection without accessing the body
                NativeApiMaintenanceBody::Fixed(not_found())
            }
        };
        Self { parts, body }
    }

    /// Invoke the prepared application only for a previously admitted request.
    pub fn serve<Pool, Codec, Values, Encoder>(
        self,
        application: &PreparedPrivateMaintenanceApi<Pool>,
        owners: EncodedApiMaintenanceRequestOwners<'_, Codec, Values>,
        encoder: &mut Encoder,
    ) -> Result<Response<NativeHttpBody>, http::Error>
    where
        Pool: WeatherApiMaintenancePool + WeatherApiSharedPool,
        Codec: ApiMaintenanceRequestBodyCodec,
        Values: ApiMaintenanceShadowEvidenceValues + ApiMaintenanceRevisionEvidenceValues,
        Encoder: AdjustmentMaintenanceWireEncoder,
    {
        let request = DecodedAdjustmentMaintenanceRequest {
            method: self.parts.method.as_str(),
            pathname: origin_form_target(&self.parts),
        };
        let wire = match self.body {
            NativeApiMaintenanceBody::Application(bytes) => {
                application.serve_encoded(request, &bytes, owners, encoder)
            }
            NativeApiMaintenanceBody::Fixed(response) => {
                // preserve HEAD representation length while suppressing its body bytes
                materialize_adjustment_maintenance_wire_response(
                    response,
                    request.method == "HEAD",
                    encoder,
                )
            }
        };
        native_maintenance_http_response(wire)
    }
}

/// Preserve an exact origin-form target and refuse absolute-form aliases.
fn origin_form_target(parts: &Parts) -> &str {
    // the private source compares the complete raw target including any query delimiter
    if parts.uri.scheme().is_some() || parts.uri.authority().is_some() {
        return "";
    }
    parts
        .uri
        .path_and_query()
        .map_or("", |target| target.as_str())
}

/// Construct the fixed pre-dispatch route refusal through the shared response mapper.
fn not_found() -> AdjustmentMaintenanceResponse {
    AdjustmentMaintenanceResponse {
        status: 404,
        body: AdjustmentMaintenanceResponseBody::Error(AdjustmentMaintenanceErrorCode::NotFound),
    }
}

/// Construct the one redacted request-body failure through the shared response mapper.
fn shadow_rejected() -> AdjustmentMaintenanceResponse {
    AdjustmentMaintenanceResponse {
        status: 409,
        body: AdjustmentMaintenanceResponseBody::Error(
            AdjustmentMaintenanceErrorCode::ShadowRejected,
        ),
    }
}
