//! Concrete primitive-owned wire adapter for worker-to-API maintenance posts.

use weather_contracts::JsonValue;
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, commit_receipt_value, fixed_gauge_gap_input_value,
    fixed_gauge_status_input_value, rain_control_gap_input_value, revision_gap_input_value,
    revision_stage_receipt_value, shadow_gap_input_value, shadow_stage_receipt_value,
};

use crate::worker_rain_maintenance_http::{
    AdmittedWorkerRainMaintenanceOrigin, WorkerRainMaintenanceHttpJson,
    WorkerRainMaintenanceHttpOutcome, WorkerRainMaintenancePost, WorkerRainMaintenanceRequest,
    WorkerRainMaintenanceResponse,
};

/// One admitted private POST after ordered JSON encoding and request-size admission.
pub struct WorkerRainMaintenanceRawPost<'a> {
    pub origin: &'a AdmittedWorkerRainMaintenanceOrigin,
    pub path: &'static str,
    pub content_type: &'static str,
    pub maximum_response_body_bytes: usize,
    pub body: &'a [u8],
}

/// One raw private response retained until status classification and typed decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRainMaintenanceRawResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Minimal private POST and capacity-delay effects for one already-admitted origin.
pub trait WorkerRainMaintenancePostTransport {
    type Error;

    /// Send the exact bounded bytes and return one bounded raw response.
    fn post(
        &mut self,
        request: WorkerRainMaintenanceRawPost<'_>,
    ) -> Result<WorkerRainMaintenanceRawResponse, Self::Error>;

    /// Wait once before the client retries an explicit archive-capacity response.
    fn wait_for_revision_archive_capacity(&mut self, milliseconds: u64) -> Result<(), Self::Error>;
}

/// Closed categorical failures around the injected primitive and transport effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRainMaintenanceWireError<TransportError> {
    Primitive,
    RequestBodyTooLarge,
    ResponseBodyTooLarge,
    Transport(TransportError),
    InvalidResponse,
}

/// Concrete base64, ordered-JSON, response-decoding, and raw-transport owner.
pub struct NativeWorkerRainMaintenanceHttpJson<P, Transport> {
    codec: ArchiveWireCodec<P>,
    transport: Transport,
}

impl<P: ArchiveWirePrimitives, Transport> NativeWorkerRainMaintenanceHttpJson<P, Transport> {
    /// Bind one approved primitive owner and raw private POST transport.
    pub const fn new(primitives: P, transport: Transport) -> Self {
        Self {
            codec: ArchiveWireCodec::new(primitives),
            transport,
        }
    }

    /// Return both injected owners after process shutdown or fixture completion.
    pub fn into_parts(self) -> (ArchiveWireCodec<P>, Transport) {
        (self.codec, self.transport)
    }
}

impl<P, Transport> WorkerRainMaintenanceHttpJson
    for NativeWorkerRainMaintenanceHttpJson<P, Transport>
where
    P: ArchiveWirePrimitives,
    Transport: WorkerRainMaintenancePostTransport,
{
    type Error = WorkerRainMaintenanceWireError<Transport::Error>;

    /// Encode exact bytes through the single admitted standard-base64 primitive.
    fn base64(&mut self, bytes: &[u8]) -> Result<String, Self::Error> {
        self.codec
            .primitives()
            .encode_standard_base64(bytes)
            .ok_or(WorkerRainMaintenanceWireError::Primitive)
    }

    /// Serialize one exact request, classify status first, then decode its typed success body.
    fn post_json(
        &mut self,
        post: WorkerRainMaintenancePost<'_>,
    ) -> Result<WorkerRainMaintenanceHttpOutcome, Self::Error> {
        let request_kind = post.body;
        let value = request_value(request_kind);
        let mut body = self
            .codec
            .primitives()
            .encode_ordered_json(&value)
            .ok_or(WorkerRainMaintenanceWireError::Primitive)?;
        // remove only the archive codec's documented framing LF to match JSON.stringify
        if body.pop() != Some(b'\n') {
            return Err(WorkerRainMaintenanceWireError::Primitive);
        }
        // reject oversized encoded requests before the raw transport can observe them
        if body.len() > post.maximum_body_bytes {
            return Err(WorkerRainMaintenanceWireError::RequestBodyTooLarge);
        }
        let response = self
            .transport
            .post(WorkerRainMaintenanceRawPost {
                origin: post.origin,
                path: post.path,
                content_type: post.content_type,
                maximum_response_body_bytes: post.maximum_body_bytes,
                body: &body,
            })
            .map_err(WorkerRainMaintenanceWireError::Transport)?;
        // preserve the original non-success classification before response-body work
        if !(200..300).contains(&response.status) {
            return Ok(WorkerRainMaintenanceHttpOutcome::HttpStatus(
                response.status,
            ));
        }
        // retain a symmetric response allocation ceiling for successful private replies
        if response.body.len() > post.maximum_body_bytes {
            return Err(WorkerRainMaintenanceWireError::ResponseBodyTooLarge);
        }
        let value = self
            .codec
            .primitives()
            .decode_json(&response.body)
            .ok_or(WorkerRainMaintenanceWireError::InvalidResponse)?;
        let decoded = decode_success(&self.codec, request_kind, &value)
            .ok_or(WorkerRainMaintenanceWireError::InvalidResponse)?;
        Ok(WorkerRainMaintenanceHttpOutcome::Success(decoded))
    }

    /// Delegate the client's bounded retry delay to the selected runtime transport owner.
    fn wait_for_revision_archive_capacity(&mut self, milliseconds: u64) -> Result<(), Self::Error> {
        self.transport
            .wait_for_revision_archive_capacity(milliseconds)
            .map_err(WorkerRainMaintenanceWireError::Transport)
    }
}

/// Map one typed request to the exact insertion order emitted by the original Node client.
fn request_value(request: WorkerRainMaintenanceRequest<'_>) -> JsonValue {
    match request {
        WorkerRainMaintenanceRequest::ShadowStage(input) => JsonValue::Object(vec![
            member("bodyBase64", string(input.bodyBase64)),
            member("comparatorBase64", string(input.comparatorBase64)),
            member(
                "sourceProjectionBase64",
                string(input.sourceProjectionBase64),
            ),
        ]),
        WorkerRainMaintenanceRequest::ShadowPublish(input) => JsonValue::Object(vec![
            member("bodyBase64", string(input.bodyBase64)),
            member("comparatorBase64", string(input.comparatorBase64)),
            member("predictionCommittedAt", string(input.predictionCommittedAt)),
            member(
                "revisionReceipt",
                commit_receipt_value(input.revisionReceipt),
            ),
            member(
                "sourceProjectionBase64",
                string(input.sourceProjectionBase64),
            ),
            member(
                "stageReceipt",
                shadow_stage_receipt_value(input.stageReceipt),
            ),
        ]),
        WorkerRainMaintenanceRequest::ShadowGap(input) => shadow_gap_input_value(input),
        WorkerRainMaintenanceRequest::RevisionStage(input) => JsonValue::Object(vec![member(
            "projectionBase64",
            string(input.projectionBase64),
        )]),
        WorkerRainMaintenanceRequest::RevisionPublish(input) => JsonValue::Object(vec![
            member("projectionBase64", string(input.projectionBase64)),
            member(
                "revisionReceipt",
                commit_receipt_value(input.revisionReceipt),
            ),
            member(
                "stageReceipt",
                revision_stage_receipt_value(input.stageReceipt),
            ),
        ]),
        WorkerRainMaintenanceRequest::RevisionBatchPublish(input) => JsonValue::Object(vec![
            member("projectionBase64", string(input.projectionBase64)),
            member(
                "revisionReceipts",
                JsonValue::Array(
                    input
                        .revisionReceipts
                        .iter()
                        .map(commit_receipt_value)
                        .collect(),
                ),
            ),
            member(
                "stageReceipt",
                revision_stage_receipt_value(input.stageReceipt),
            ),
        ]),
        WorkerRainMaintenanceRequest::RevisionGap(input) => revision_gap_input_value(input),
        WorkerRainMaintenanceRequest::RainControlStateStage(input) => {
            JsonValue::Object(vec![member("stateBase64", string(input.stateBase64))])
        }
        WorkerRainMaintenanceRequest::RainControlStateGap(input) => {
            rain_control_gap_input_value(input)
        }
        WorkerRainMaintenanceRequest::FixedGaugeGapStatus(input) => {
            fixed_gauge_status_input_value(input)
        }
        WorkerRainMaintenanceRequest::FixedGaugeGap(input) => fixed_gauge_gap_input_value(input),
    }
}

/// Decode one successful JSON value through the shared receipt contract for its request kind.
fn decode_success<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    request: WorkerRainMaintenanceRequest<'_>,
    value: &JsonValue,
) -> Option<WorkerRainMaintenanceResponse> {
    match request {
        WorkerRainMaintenanceRequest::ShadowStage(_) => codec
            .decode_shadow_stage_value(value)
            .map(WorkerRainMaintenanceResponse::ShadowStage),
        WorkerRainMaintenanceRequest::RevisionStage(_) => codec
            .decode_revision_stage_value(value)
            .map(WorkerRainMaintenanceResponse::RevisionStage),
        WorkerRainMaintenanceRequest::RainControlStateStage(_) => codec
            .decode_rain_stage_value(value)
            .map(WorkerRainMaintenanceResponse::RainControlStateStage),
        WorkerRainMaintenanceRequest::FixedGaugeGapStatus(_) => codec
            .decode_fixed_gauge_status_value(value)
            .map(WorkerRainMaintenanceResponse::FixedGaugeGapStatus),
        WorkerRainMaintenanceRequest::FixedGaugeGap(_) => codec
            .decode_fixed_gauge_gap_value(value)
            .map(WorkerRainMaintenanceResponse::FixedGaugeGap),
        WorkerRainMaintenanceRequest::ShadowPublish(_)
        | WorkerRainMaintenanceRequest::ShadowGap(_)
        | WorkerRainMaintenanceRequest::RevisionPublish(_)
        | WorkerRainMaintenanceRequest::RevisionBatchPublish(_)
        | WorkerRainMaintenanceRequest::RevisionGap(_)
        | WorkerRainMaintenanceRequest::RainControlStateGap(_) => {
            Some(WorkerRainMaintenanceResponse::ParsedJson)
        }
    }
}

/// Represent one exact UTF-16 JSON string through the generated contract value.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Represent one exact producer-ordered JSON object member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}
