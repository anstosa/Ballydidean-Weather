//! Concrete primitive-owned wire adapter for API-to-web maintenance archive posts.

use weather_api::AdmittedMaintenanceArchiveOrigin;
use weather_contracts::JsonValue;
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, commit_receipt_value, fixed_gauge_gap_input_value,
    fixed_gauge_status_input_value, rain_control_gap_input_value, revision_gap_input_value,
    revision_stage_receipt_value, shadow_gap_input_value, shadow_metadata_value,
    shadow_stage_receipt_value,
};

use crate::api_maintenance_archive_relay::{
    ApiMaintenanceArchiveHttpJson, ApiMaintenanceArchiveHttpOutcome, ApiMaintenanceArchivePost,
    ApiMaintenanceArchiveRequest, ApiMaintenanceArchiveResponse,
};

/// One admitted private POST after ordered JSON encoding and request-size admission.
pub struct ApiMaintenanceArchiveRawPost<'a> {
    pub origin: &'a AdmittedMaintenanceArchiveOrigin,
    pub path: &'static str,
    pub content_type: &'static str,
    pub maximum_response_body_bytes: usize,
    pub body: &'a [u8],
}

/// One raw private response retained until status classification and typed decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiMaintenanceArchiveRawResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Minimal POST-only transport for one already-admitted private archive origin.
pub trait ApiMaintenanceArchivePostTransport {
    type Error;

    /// Send the exact bounded bytes and return one bounded raw response.
    fn post(
        &mut self,
        request: ApiMaintenanceArchiveRawPost<'_>,
    ) -> Result<ApiMaintenanceArchiveRawResponse, Self::Error>;
}

/// Closed categorical failures around the injected primitive and transport effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiMaintenanceArchiveWireError<TransportError> {
    Primitive,
    RequestBodyTooLarge,
    ResponseBodyTooLarge,
    Transport(TransportError),
    InvalidResponse,
}

/// Concrete base64, ordered-JSON, response-decoding, and raw-transport owner.
pub struct NativeApiMaintenanceArchiveHttpJson<P, Transport> {
    codec: ArchiveWireCodec<P>,
    transport: Transport,
}

impl<P: ArchiveWirePrimitives, Transport> NativeApiMaintenanceArchiveHttpJson<P, Transport> {
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

impl<P, Transport> ApiMaintenanceArchiveHttpJson
    for NativeApiMaintenanceArchiveHttpJson<P, Transport>
where
    P: ArchiveWirePrimitives,
    Transport: ApiMaintenanceArchivePostTransport,
{
    type Error = ApiMaintenanceArchiveWireError<Transport::Error>;

    /// Encode exact bytes through the single admitted standard-base64 primitive.
    fn base64(&mut self, bytes: &[u8]) -> Result<String, Self::Error> {
        self.codec
            .primitives()
            .encode_standard_base64(bytes)
            .ok_or(ApiMaintenanceArchiveWireError::Primitive)
    }

    /// Serialize one exact request, classify status first, then decode its typed success body.
    fn post_json(
        &mut self,
        post: ApiMaintenanceArchivePost<'_>,
    ) -> Result<ApiMaintenanceArchiveHttpOutcome, Self::Error> {
        let request_kind = post.body;
        let value = request_value(request_kind);
        let mut body = self
            .codec
            .primitives()
            .encode_ordered_json(&value)
            .ok_or(ApiMaintenanceArchiveWireError::Primitive)?;
        // remove only the archive codec's documented framing LF to match JSON.stringify
        if body.pop() != Some(b'\n') {
            return Err(ApiMaintenanceArchiveWireError::Primitive);
        }
        // reject oversized encoded requests before the raw transport can observe them
        if body.len() > post.maximum_body_bytes {
            return Err(ApiMaintenanceArchiveWireError::RequestBodyTooLarge);
        }
        let response = self
            .transport
            .post(ApiMaintenanceArchiveRawPost {
                origin: post.origin,
                path: post.path,
                content_type: post.content_type,
                maximum_response_body_bytes: post.maximum_body_bytes,
                body: &body,
            })
            .map_err(ApiMaintenanceArchiveWireError::Transport)?;
        // preserve Node's non-success classification before any response-body work
        if !(200..300).contains(&response.status) {
            return Ok(ApiMaintenanceArchiveHttpOutcome::HttpStatus(
                response.status,
            ));
        }
        // retain a symmetric response allocation ceiling for successful private replies
        if response.body.len() > post.maximum_body_bytes {
            return Err(ApiMaintenanceArchiveWireError::ResponseBodyTooLarge);
        }
        let value = self
            .codec
            .primitives()
            .decode_json(&response.body)
            .ok_or(ApiMaintenanceArchiveWireError::InvalidResponse)?;
        let decoded = decode_success(&self.codec, request_kind, &value)
            .ok_or(ApiMaintenanceArchiveWireError::InvalidResponse)?;
        Ok(ApiMaintenanceArchiveHttpOutcome::Success(decoded))
    }
}

/// Map one typed request to the exact insertion order emitted by the original Node relay.
fn request_value(request: ApiMaintenanceArchiveRequest<'_>) -> JsonValue {
    match request {
        ApiMaintenanceArchiveRequest::ShadowStage(input) => {
            let mut fields = vec![member("bodyBase64", string(input.bodyBase64))];
            // preserve comparator absence as the original V1 body discriminator
            if let Some(comparator) = input.comparatorBase64 {
                fields.push(member("comparatorBase64", string(comparator)));
            }
            fields.extend([
                member("metadata", shadow_metadata_value(input.metadata)),
                member(
                    "sourceProjectionBase64",
                    string(input.sourceProjectionBase64),
                ),
                member(
                    "sourceProjectionSha256",
                    string(input.sourceProjectionSha256),
                ),
            ]);
            JsonValue::Object(fields)
        }
        ApiMaintenanceArchiveRequest::ShadowPublish(input) => {
            // retain the rejected predecessor v1 envelope unchanged
            let mut fields = vec![member("bodyBase64", string(input.bodyBase64))];
            // preserve comparator absence while retaining the original conditional position
            if let Some(comparator) = input.comparatorBase64 {
                fields.push(member("comparatorBase64", string(comparator)));
            }
            fields.push(member("metadata", shadow_metadata_value(input.metadata)));
            // preserve the original optional database commit clock
            if let Some(committed_at) = input.predictionCommittedAt {
                fields.push(member("predictionCommittedAt", string(committed_at)));
            }
            fields.extend([
                member(
                    "revisionReceipt",
                    commit_receipt_value(input.revisionReceipt),
                ),
                member(
                    "sourceProjectionBase64",
                    string(input.sourceProjectionBase64),
                ),
                member(
                    "sourceProjectionSha256",
                    string(input.sourceProjectionSha256),
                ),
                member(
                    "stageReceipt",
                    shadow_stage_receipt_value(input.stageReceipt),
                ),
            ]);
            JsonValue::Object(fields)
        }
        ApiMaintenanceArchiveRequest::ShadowGap(input) => shadow_gap_input_value(input),
        ApiMaintenanceArchiveRequest::RevisionStage(input) => JsonValue::Object(vec![member(
            "projectionBase64",
            string(input.projectionBase64),
        )]),
        ApiMaintenanceArchiveRequest::RevisionPublish(input) => JsonValue::Object(vec![
            member(
                "revisionReceipt",
                commit_receipt_value(input.revisionReceipt),
            ),
            member(
                "stageReceipt",
                revision_stage_receipt_value(input.stageReceipt),
            ),
        ]),
        ApiMaintenanceArchiveRequest::RevisionBatchPublish(input) => JsonValue::Object(vec![
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
        ApiMaintenanceArchiveRequest::RevisionGap(input) => revision_gap_input_value(input),
        ApiMaintenanceArchiveRequest::RainControlStateStage(input) => {
            JsonValue::Object(vec![member("stateBase64", string(input.stateBase64))])
        }
        ApiMaintenanceArchiveRequest::RainControlStateGap(input) => {
            rain_control_gap_input_value(input)
        }
        ApiMaintenanceArchiveRequest::RainFixedGaugeTargetGap(input) => {
            fixed_gauge_gap_input_value(input)
        }
        ApiMaintenanceArchiveRequest::RainFixedGaugeTargetGapStatus(input) => {
            fixed_gauge_status_input_value(input)
        }
    }
}

/// Decode one successful JSON value through the shared edge receipt contract for its request kind.
fn decode_success<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    request: ApiMaintenanceArchiveRequest<'_>,
    value: &JsonValue,
) -> Option<ApiMaintenanceArchiveResponse> {
    match request {
        ApiMaintenanceArchiveRequest::ShadowStage(_) => codec
            .decode_shadow_stage_value(value)
            .map(ApiMaintenanceArchiveResponse::ShadowStage),
        ApiMaintenanceArchiveRequest::ShadowPublish(_) => codec
            .decode_shadow_publish_value(value)
            .map(ApiMaintenanceArchiveResponse::ShadowPublish),
        ApiMaintenanceArchiveRequest::ShadowGap(_)
        | ApiMaintenanceArchiveRequest::RevisionGap(_)
        | ApiMaintenanceArchiveRequest::RainControlStateGap(_) => {
            Some(ApiMaintenanceArchiveResponse::ParsedGapJson)
        }
        ApiMaintenanceArchiveRequest::RevisionStage(_) => codec
            .decode_revision_stage_value(value)
            .map(ApiMaintenanceArchiveResponse::RevisionStage),
        ApiMaintenanceArchiveRequest::RevisionPublish(_) => codec
            .decode_revision_publish_value(value)
            .map(ApiMaintenanceArchiveResponse::RevisionPublish),
        ApiMaintenanceArchiveRequest::RevisionBatchPublish(_) => codec
            .decode_revision_batch_publish_value(value)
            .map(ApiMaintenanceArchiveResponse::RevisionBatchPublish),
        ApiMaintenanceArchiveRequest::RainControlStateStage(_) => codec
            .decode_rain_stage_value(value)
            .map(ApiMaintenanceArchiveResponse::RainControlStateStage),
        ApiMaintenanceArchiveRequest::RainFixedGaugeTargetGap(_) => codec
            .decode_fixed_gauge_gap_value(value)
            .map(ApiMaintenanceArchiveResponse::RainFixedGaugeTargetGap),
        ApiMaintenanceArchiveRequest::RainFixedGaugeTargetGapStatus(_) => codec
            .decode_fixed_gauge_status_value(value)
            .map(ApiMaintenanceArchiveResponse::RainFixedGaugeTargetGapStatus),
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
