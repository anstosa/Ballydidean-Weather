//! Materialize private maintenance outcomes with the original Node JSON wire metadata.

use crate::maintenance_service::{
    AdjustmentMaintenanceBodyRead, AdjustmentMaintenanceErrorCode, AdjustmentMaintenanceRepository,
    AdjustmentMaintenanceResponse, AdjustmentMaintenanceResponseBody,
    AdjustmentMaintenanceScheduleRepository, AdjustmentMaintenanceServicePorts,
    AdmittedAdjustmentMaintenanceAuthority, DecodedAdjustmentMaintenanceRequest,
    UnifiedAdjustmentMaintenanceServicePorts, serve_adjustment_maintenance_request,
    serve_adjustment_maintenance_request_with_repository,
};

const JSON_CONTENT_TYPE: &str = "application/json";
const ACCEPTED_JSON: &[u8] = b"{\"accepted\":true}";
const RECORDED_JSON: &[u8] = b"{\"recorded\":true}";
const NOT_FOUND_JSON: &[u8] = b"{\"error\":\"not_found\"}";
const REVISION_SPOOL_FULL_JSON: &[u8] = b"{\"error\":\"revision_spool_full\"}";
const SHADOW_REJECTED_JSON: &[u8] = b"{\"error\":\"shadow_rejected\"}";

/// Body storage selected without copying fixed or encoder-owned bytes.
#[derive(Debug, PartialEq, Eq)]
pub enum AdjustmentMaintenanceWireBody<'a> {
    Empty,
    Borrowed(&'a [u8]),
}

impl AdjustmentMaintenanceWireBody<'_> {
    /// Borrow the emitted bytes without changing their storage owner.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Empty => &[],
            Self::Borrowed(bytes) => bytes,
        }
    }

    /// Report whether transport suppression removed the representation body.
    pub fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }
}

/// Exact headers emitted by the private Node `writeJson` helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdjustmentMaintenanceWireHeaders {
    pub content_length: usize,
    pub content_type: &'static str,
}

/// Complete transport-independent private maintenance response.
#[derive(Debug, PartialEq, Eq)]
pub struct AdjustmentMaintenanceWireResponse<'a> {
    pub body: AdjustmentMaintenanceWireBody<'a>,
    pub headers: AdjustmentMaintenanceWireHeaders,
    pub status_code: u16,
}

/// Approved serializer scratch for dynamic maintenance receipts and schedule slots.
pub trait AdjustmentMaintenanceWireEncoder {
    type Error;

    /// Encode one typed dynamic body without a trailing line feed.
    fn encode<'a>(
        &'a mut self,
        body: &AdjustmentMaintenanceResponseBody,
    ) -> Result<&'a [u8], Self::Error>;
}

/// Dispatch one decoded request and materialize its exact private JSON representation.
pub fn serve_adjustment_maintenance_wire_request<'a, Encoder>(
    authority: AdmittedAdjustmentMaintenanceAuthority,
    repository: &mut dyn AdjustmentMaintenanceScheduleRepository,
    body: &mut dyn AdjustmentMaintenanceBodyRead,
    ports: AdjustmentMaintenanceServicePorts<'_, '_, '_, '_, '_>,
    request: DecodedAdjustmentMaintenanceRequest<'_>,
    encoder: &'a mut Encoder,
) -> AdjustmentMaintenanceWireResponse<'a>
where
    Encoder: AdjustmentMaintenanceWireEncoder,
{
    let omit_body = request.method == "HEAD";
    let response =
        serve_adjustment_maintenance_request(authority, repository, body, ports, request);
    materialize_adjustment_maintenance_wire_response(response, omit_body, encoder)
}

/// Dispatch and encode one request through a single schedule and publication repository.
pub fn serve_adjustment_maintenance_wire_request_with_repository<'a, Encoder>(
    authority: AdmittedAdjustmentMaintenanceAuthority,
    repository: &mut dyn AdjustmentMaintenanceRepository,
    body: &mut dyn AdjustmentMaintenanceBodyRead,
    ports: UnifiedAdjustmentMaintenanceServicePorts<'_, '_, '_, '_>,
    request: DecodedAdjustmentMaintenanceRequest<'_>,
    encoder: &'a mut Encoder,
) -> AdjustmentMaintenanceWireResponse<'a>
where
    Encoder: AdjustmentMaintenanceWireEncoder,
{
    let omit_body = request.method == "HEAD";
    let response = serve_adjustment_maintenance_request_with_repository(
        authority, repository, body, ports, request,
    );
    materialize_adjustment_maintenance_wire_response(response, omit_body, encoder)
}

/// Serialize one typed service outcome before any private response headers are emitted.
pub fn materialize_adjustment_maintenance_wire_response<'a, Encoder>(
    response: AdjustmentMaintenanceResponse,
    omit_body: bool,
    encoder: &'a mut Encoder,
) -> AdjustmentMaintenanceWireResponse<'a>
where
    Encoder: AdjustmentMaintenanceWireEncoder,
{
    // retain fixed source JSON without invoking the scratch encoder
    if let Some(representation) = fixed_json(&response.body) {
        return wire_response(response.status, representation, omit_body);
    }

    // collapse a pre-writer encoding refusal through the source catch response
    match encoder.encode(&response.body) {
        Ok(representation) => wire_response(response.status, representation, omit_body),
        Err(_) => wire_response(409, SHADOW_REJECTED_JSON, omit_body),
    }
}

/// Bind complete representation bytes to metadata before optional body suppression.
fn wire_response(
    status_code: u16,
    representation: &[u8],
    omit_body: bool,
) -> AdjustmentMaintenanceWireResponse<'_> {
    let content_length = representation.len();
    let body = if omit_body {
        AdjustmentMaintenanceWireBody::Empty
    } else {
        AdjustmentMaintenanceWireBody::Borrowed(representation)
    };
    AdjustmentMaintenanceWireResponse {
        body,
        headers: AdjustmentMaintenanceWireHeaders {
            content_length,
            content_type: JSON_CONTENT_TYPE,
        },
        status_code,
    }
}

/// Select one source-owned representation that never requires dynamic encoding.
const fn fixed_json(body: &AdjustmentMaintenanceResponseBody) -> Option<&'static [u8]> {
    match body {
        AdjustmentMaintenanceResponseBody::Accepted => Some(ACCEPTED_JSON),
        AdjustmentMaintenanceResponseBody::Recorded => Some(RECORDED_JSON),
        AdjustmentMaintenanceResponseBody::Error(error) => Some(error_json(*error)),
        _ => None,
    }
}

/// Select one closed fixed error representation from the typed service category.
const fn error_json(error: AdjustmentMaintenanceErrorCode) -> &'static [u8] {
    match error {
        AdjustmentMaintenanceErrorCode::NotFound => NOT_FOUND_JSON,
        AdjustmentMaintenanceErrorCode::RevisionSpoolFull => REVISION_SPOOL_FULL_JSON,
        AdjustmentMaintenanceErrorCode::ShadowRejected => SHADOW_REJECTED_JSON,
    }
}

#[cfg(test)]
mod tests {
    use weather_contracts::{
        RainFixedGaugeTargetGapStatus, RainFixedGaugeTargetGapStatusAbsent,
        RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
        RainFixedGaugeTargetGapStatusAbsentStateLiteral,
    };

    use super::*;

    const STATUS_JSON: &[u8] =
        b"{\"contractVersion\":\"adjustment-rain-fixed-gauge-target-gap-status/v1\",\"state\":\"absent\"}";

    #[derive(Default)]
    struct Encoder {
        calls: usize,
        refuse: bool,
        scratch: Vec<u8>,
    }

    impl AdjustmentMaintenanceWireEncoder for Encoder {
        type Error = ();

        /// Encode or refuse the fixture's typed fixed-gauge status into borrowed scratch.
        fn encode<'a>(
            &'a mut self,
            body: &AdjustmentMaintenanceResponseBody,
        ) -> Result<&'a [u8], Self::Error> {
            self.calls += 1;
            assert!(matches!(
                body,
                AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGapStatus(
                    RainFixedGaugeTargetGapStatus::Absent(_)
                )
            ));
            // inject one ordinary serializer refusal before writer admission
            if self.refuse {
                return Err(());
            }
            self.scratch.clear();
            self.scratch.extend_from_slice(STATUS_JSON);
            Ok(&self.scratch)
        }
    }

    /// Preserve fixed accepted, recorded, and closed error bytes without line feeds.
    #[test]
    fn fixed_representations_match_private_write_json() {
        let mut encoder = Encoder::default();
        // verify each source-owned fixed representation and status independently
        for (status_code, body, expected) in [
            (
                202,
                AdjustmentMaintenanceResponseBody::Accepted,
                ACCEPTED_JSON,
            ),
            (
                200,
                AdjustmentMaintenanceResponseBody::Recorded,
                RECORDED_JSON,
            ),
            (
                404,
                AdjustmentMaintenanceResponseBody::Error(AdjustmentMaintenanceErrorCode::NotFound),
                NOT_FOUND_JSON,
            ),
            (
                429,
                AdjustmentMaintenanceResponseBody::Error(
                    AdjustmentMaintenanceErrorCode::RevisionSpoolFull,
                ),
                REVISION_SPOOL_FULL_JSON,
            ),
            (
                409,
                AdjustmentMaintenanceResponseBody::Error(
                    AdjustmentMaintenanceErrorCode::ShadowRejected,
                ),
                SHADOW_REJECTED_JSON,
            ),
        ] {
            let response = materialize_adjustment_maintenance_wire_response(
                AdjustmentMaintenanceResponse {
                    status: status_code,
                    body,
                },
                false,
                &mut encoder,
            );
            assert_eq!(response.status_code, status_code);
            assert_eq!(response.body.as_bytes(), expected);
            assert_eq!(response.headers.content_length, expected.len());
            assert_eq!(response.headers.content_type, "application/json");
            assert!(!expected.ends_with(b"\n"));
        }
        assert_eq!(encoder.calls, 0);
    }

    /// Use the typed encoder for a dynamic receipt and retain its exact scratch bytes.
    #[test]
    fn dynamic_status_uses_borrowed_encoder_bytes() {
        let mut encoder = Encoder::default();
        let response = materialize_adjustment_maintenance_wire_response(
            status_response(),
            false,
            &mut encoder,
        );
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.as_bytes(), STATUS_JSON);
        assert_eq!(response.headers.content_length, STATUS_JSON.len());
        assert_eq!(encoder.calls, 1);
    }

    /// Collapse serializer refusal before headers and retain the omitted representation length.
    #[test]
    fn encoder_refusal_and_body_suppression_use_shadow_rejected() {
        let mut encoder = Encoder {
            refuse: true,
            ..Encoder::default()
        };
        let response =
            materialize_adjustment_maintenance_wire_response(status_response(), true, &mut encoder);
        assert_eq!(response.status_code, 409);
        assert!(response.body.is_empty());
        assert_eq!(response.headers.content_length, SHADOW_REJECTED_JSON.len());
        assert_eq!(encoder.calls, 1);
    }

    /// Preserve a fixed legacy head representation length while omitting only its body.
    #[test]
    fn fixed_head_response_retains_would_be_length() {
        let mut encoder = Encoder::default();
        let response = materialize_adjustment_maintenance_wire_response(
            AdjustmentMaintenanceResponse {
                status: 404,
                body: AdjustmentMaintenanceResponseBody::Error(
                    AdjustmentMaintenanceErrorCode::NotFound,
                ),
            },
            true,
            &mut encoder,
        );
        assert!(response.body.is_empty());
        assert_eq!(response.headers.content_length, NOT_FOUND_JSON.len());
        assert_eq!(encoder.calls, 0);
    }

    /// Build one admitted fixed-gauge status owned by the dynamic encoder.
    fn status_response() -> AdjustmentMaintenanceResponse {
        AdjustmentMaintenanceResponse {
            status: 200,
            body: AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGapStatus(
                RainFixedGaugeTargetGapStatus::Absent(RainFixedGaugeTargetGapStatusAbsent {
                    contractVersion: RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
                    state: RainFixedGaugeTargetGapStatusAbsentStateLiteral,
                }),
            ),
        }
    }
}
