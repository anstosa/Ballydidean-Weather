//! Retained bounded request bodies for the private adjustment-maintenance API.

use weather_api::{
    AdjustmentMaintenanceBodyError, AdjustmentMaintenanceBodyRead,
    DecodedAdjustmentMaintenancePublish, DecodedAdjustmentMaintenanceStage,
    DecodedAdjustmentRevisionPublish, DecodedAdjustmentRevisionStage,
    DecodedRainMaintenanceControlStateStage, MAX_ADJUSTMENT_MAINTENANCE_REQUEST_BYTES,
};
use weather_contracts::{
    AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceGapInput,
    AdjustmentMaintenanceGapInputFamilyEnum, AdjustmentMaintenanceStageReceipt,
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionGapInput, AdjustmentRevisionGapReason,
    AdjustmentRevisionProjectionKind, AdjustmentRevisionStageReceipt, FieldPresence, JsonValue,
    MaintenanceShadowGapReason, Nullable, RainFixedGaugeTargetGapInput,
    RainFixedGaugeTargetGapInputReasonLiteral, RainFixedGaugeTargetGapStatusInput,
    RainMaintenanceControlStateGapInput, RainMaintenanceControlStateGapInputReasonLiteral,
};
use weather_edge::{ArchiveWireCodec, ArchiveWirePrimitives};

use crate::maintenance_canonical_json::{exact_keys, field, text};

/// Exact Node private request and encoded base64 character ceiling.
pub const MAXIMUM_API_MAINTENANCE_REQUEST_BYTES: usize = MAX_ADJUSTMENT_MAINTENANCE_REQUEST_BYTES;

const CAPTURE_FIELDS: &[&str] = &["dueKey", "issuedAt"];
const GAP_FIELDS: &[&str] = &["dueKey", "family", "reason", "registrationSha256"];
const SHADOW_STAGE_FIELDS: &[&str] = &["bodyBase64", "sourceProjectionBase64"];
const SHADOW_STAGE_COMPARATOR_FIELDS: &[&str] =
    &["bodyBase64", "comparatorBase64", "sourceProjectionBase64"];
const SHADOW_PUBLISH_FIELDS: &[&str] = &[
    "bodyBase64",
    "revisionReceipt",
    "sourceProjectionBase64",
    "stageReceipt",
];
const SHADOW_PUBLISH_COMPARATOR_FIELDS: &[&str] = &[
    "bodyBase64",
    "comparatorBase64",
    "predictionCommittedAt",
    "revisionReceipt",
    "sourceProjectionBase64",
    "stageReceipt",
];
const REVISION_STAGE_FIELDS: &[&str] = &["projectionBase64"];
const REVISION_PUBLISH_FIELDS: &[&str] = &["projectionBase64", "revisionReceipt", "stageReceipt"];
const REVISION_BATCH_PUBLISH_FIELDS: &[&str] =
    &["projectionBase64", "revisionReceipts", "stageReceipt"];
const REVISION_GAP_FIELDS: &[&str] = &[
    "logicalKeySha256",
    "projectionIdentitySha256",
    "projectionKind",
    "projectionSha256",
    "reason",
];
const RAIN_CONTROL_STATE_STAGE_FIELDS: &[&str] = &["stateBase64"];
const RAIN_CONTROL_STATE_GAP_FIELDS: &[&str] = &["reason", "stateSha256", "stageReceiptSha256"];
const RAIN_FIXED_GAUGE_TARGET_GAP_FIELDS: &[&str] =
    &["logicalHourAt", "logicalKeySha256", "reason"];
const RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_FIELDS: &[&str] = &["logicalHourAt", "logicalKeySha256"];

/// Approved JSON, strict padded-base64, and closed generated-receipt decoder.
pub trait ApiMaintenanceRequestBodyCodec {
    /// Parse one bounded JSON value while retaining UTF-16 string code units.
    fn parse_json(&mut self, bytes: &[u8]) -> Option<JsonValue>;

    /// Decode one canonical padded base64 string under the exact character ceiling.
    fn decode_strict_base64(
        &mut self,
        encoded: &[u16],
        maximum_characters: usize,
    ) -> Option<Vec<u8>>;

    /// Decode one closed generated database commit receipt.
    fn decode_revision_commit_receipt(
        &mut self,
        value: &JsonValue,
    ) -> Option<AdjustmentRevisionCommitReceipt>;

    /// Decode one closed generated ordinary shadow stage receipt.
    fn decode_shadow_stage_receipt(
        &mut self,
        value: &JsonValue,
    ) -> Option<AdjustmentMaintenanceStageReceipt>;

    /// Decode one closed generated revision stage receipt.
    fn decode_revision_stage_receipt(
        &mut self,
        value: &JsonValue,
    ) -> Option<AdjustmentRevisionStageReceipt>;
}

/// Concrete worker-to-API request mapping through the shared admitted wire primitives.
pub struct NativeApiMaintenanceRequestBodyCodec<P> {
    wire: ArchiveWireCodec<P>,
}

impl<P: ArchiveWirePrimitives> NativeApiMaintenanceRequestBodyCodec<P> {
    /// Bind the selected JSON and base64 owner without copying the retained request.
    pub const fn new(primitives: P) -> Self {
        Self {
            wire: ArchiveWireCodec::new(primitives),
        }
    }
}

impl<P: ArchiveWirePrimitives> ApiMaintenanceRequestBodyCodec
    for NativeApiMaintenanceRequestBodyCodec<P>
{
    /// Decode the already bounded original JSON document through the selected primitive.
    fn parse_json(&mut self, bytes: &[u8]) -> Option<JsonValue> {
        self.wire.primitives().decode_json(bytes)
    }

    /// Preserve the original nonempty padded base64 contract before transferring decoded bytes.
    fn decode_strict_base64(
        &mut self,
        encoded: &[u16],
        maximum_characters: usize,
    ) -> Option<Vec<u8>> {
        // reject empty or over-limit values before creating a UTF-8 or decoded allocation
        if encoded.is_empty()
            || encoded.len() > maximum_characters
            || !encoded.len().is_multiple_of(4)
        {
            return None;
        }
        let text = String::from_utf16(encoded).ok()?;
        let decoded = self.wire.primitives().decode_standard_base64(&text)?;
        (self.wire.primitives().encode_standard_base64(&decoded)? == text).then_some(decoded)
    }

    /// Decode the exact generated database receipt before API repository rebinding.
    fn decode_revision_commit_receipt(
        &mut self,
        value: &JsonValue,
    ) -> Option<AdjustmentRevisionCommitReceipt> {
        self.wire.decode_revision_commit_value(value)
    }

    /// Decode one closed shadow stage receipt before full producer admission.
    fn decode_shadow_stage_receipt(
        &mut self,
        value: &JsonValue,
    ) -> Option<AdjustmentMaintenanceStageReceipt> {
        self.wire.decode_shadow_stage_value(value)
    }

    /// Decode one closed revision stage receipt before binding the actual projection.
    fn decode_revision_stage_receipt(
        &mut self,
        value: &JsonValue,
    ) -> Option<AdjustmentRevisionStageReceipt> {
        self.wire.decode_revision_stage_value(value)
    }
}

/// One bounded raw request body retained until its selected operation parses it once.
pub struct RetainedApiMaintenanceRequestBody<'a, Codec> {
    bytes: &'a [u8],
    codec: &'a mut Codec,
    decoded: bool,
}

impl<'a, Codec> RetainedApiMaintenanceRequestBody<'a, Codec> {
    /// Retain the exact bounded transport bytes and approved decoder owner.
    pub const fn new(bytes: &'a [u8], codec: &'a mut Codec) -> Self {
        Self {
            bytes,
            codec,
            decoded: false,
        }
    }
}

impl<Codec: ApiMaintenanceRequestBodyCodec> RetainedApiMaintenanceRequestBody<'_, Codec> {
    /// Parse the selected body once and collapse codec details into the private API error.
    fn decode<Value>(
        &mut self,
        decode: impl FnOnce(&mut Codec, &JsonValue) -> Option<Value>,
    ) -> Result<Value, AdjustmentMaintenanceBodyError> {
        // preserve the transport cap even when composition supplies an incorrectly retained body
        if self.decoded || self.bytes.len() > MAXIMUM_API_MAINTENANCE_REQUEST_BYTES {
            return Err(AdjustmentMaintenanceBodyError);
        }
        self.decoded = true;
        let value = self
            .codec
            .parse_json(self.bytes)
            .ok_or(AdjustmentMaintenanceBodyError)?;
        decode(self.codec, &value).ok_or(AdjustmentMaintenanceBodyError)
    }
}

impl<Codec: ApiMaintenanceRequestBodyCodec> AdjustmentMaintenanceBodyRead
    for RetainedApiMaintenanceRequestBody<'_, Codec>
{
    /// Report the exact raw byte count before any JSON or base64 decoding.
    fn encoded_len(&self) -> usize {
        self.bytes.len()
    }

    /// Decode the exact scalar scheduler capture object once.
    fn capture(
        &mut self,
    ) -> Result<AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceBodyError> {
        self.decode(|_, value| {
            exact_keys(value, CAPTURE_FIELDS).then_some(())?;
            Some(AdjustmentMaintenanceCaptureRequest {
                dueKey: text(value, "dueKey")?,
                issuedAt: text(value, "issuedAt")?,
            })
        })
    }

    /// Decode the exact scalar categorical gap object once.
    fn gap(&mut self) -> Result<AdjustmentMaintenanceGapInput, AdjustmentMaintenanceBodyError> {
        self.decode(|_, value| {
            exact_keys(value, GAP_FIELDS).then_some(())?;
            Some(AdjustmentMaintenanceGapInput {
                dueKey: text(value, "dueKey")?,
                family: wire_value(
                    value,
                    "family",
                    AdjustmentMaintenanceGapInputFamilyEnum::WIRE_VALUES,
                )?,
                reason: wire_value(value, "reason", MaintenanceShadowGapReason::WIRE_VALUES)?,
                registrationSha256: text(value, "registrationSha256")?,
            })
        })
    }

    /// Decode ordinary shadow stage bytes with exact comparator omission semantics.
    fn shadow_stage(
        &mut self,
    ) -> Result<DecodedAdjustmentMaintenanceStage, AdjustmentMaintenanceBodyError> {
        self.decode(|codec, value| {
            let comparator = optional_base64(
                codec,
                value,
                "comparatorBase64",
                SHADOW_STAGE_FIELDS,
                SHADOW_STAGE_COMPARATOR_FIELDS,
            )?;
            Some(DecodedAdjustmentMaintenanceStage {
                body: base64(codec, value, "bodyBase64")?,
                comparator,
                source_projection: base64(codec, value, "sourceProjectionBase64")?,
            })
        })
    }

    /// Decode ordinary shadow publish bytes and closed generated receipts once.
    fn shadow_publish(
        &mut self,
    ) -> Result<DecodedAdjustmentMaintenancePublish, AdjustmentMaintenanceBodyError> {
        self.decode(|codec, value| {
            let comparator = optional_base64(
                codec,
                value,
                "comparatorBase64",
                SHADOW_PUBLISH_FIELDS,
                SHADOW_PUBLISH_COMPARATOR_FIELDS,
            )?;
            let prediction_committed_at = match &comparator {
                FieldPresence::Missing => FieldPresence::Missing,
                FieldPresence::Value(_) => {
                    FieldPresence::Value(text(value, "predictionCommittedAt")?)
                }
            };
            Some(DecodedAdjustmentMaintenancePublish {
                body: base64(codec, value, "bodyBase64")?,
                comparator,
                prediction_committed_at,
                revision_receipt: codec
                    .decode_revision_commit_receipt(field(value, "revisionReceipt")?)?,
                source_projection: base64(codec, value, "sourceProjectionBase64")?,
                stage_receipt: codec.decode_shadow_stage_receipt(field(value, "stageReceipt")?)?,
            })
        })
    }

    /// Decode one exact revision stage projection once.
    fn revision_stage(
        &mut self,
    ) -> Result<DecodedAdjustmentRevisionStage, AdjustmentMaintenanceBodyError> {
        self.decode(|codec, value| {
            exact_keys(value, REVISION_STAGE_FIELDS).then_some(())?;
            Some(DecodedAdjustmentRevisionStage {
                projection: base64(codec, value, "projectionBase64")?,
            })
        })
    }

    /// Decode one exact single or grouped revision publication once.
    fn revision_publish(
        &mut self,
    ) -> Result<DecodedAdjustmentRevisionPublish, AdjustmentMaintenanceBodyError> {
        self.decode(|codec, value| {
            // select grouped publication only from the exact grouped receipt member
            if field(value, "revisionReceipts").is_some() {
                // refuse extension fields before decoding any grouped member
                if !exact_keys(value, REVISION_BATCH_PUBLISH_FIELDS) {
                    return None;
                }
                let JsonValue::Array(receipts) = field(value, "revisionReceipts")? else {
                    return None;
                };
                let mut decoded = Vec::with_capacity(receipts.len());
                // preserve the original receipt order for grouped database admission
                for receipt in receipts {
                    decoded.push(codec.decode_revision_commit_receipt(receipt)?);
                }
                return Some(DecodedAdjustmentRevisionPublish::Batch {
                    projection: base64(codec, value, "projectionBase64")?,
                    revision_receipts: decoded,
                    stage_receipt: codec
                        .decode_revision_stage_receipt(field(value, "stageReceipt")?)?,
                });
            }
            // refuse extension fields before decoding either single receipt
            if !exact_keys(value, REVISION_PUBLISH_FIELDS) {
                return None;
            }
            Some(DecodedAdjustmentRevisionPublish::Single {
                projection: base64(codec, value, "projectionBase64")?,
                revision_receipt: codec
                    .decode_revision_commit_receipt(field(value, "revisionReceipt")?)?,
                stage_receipt: codec
                    .decode_revision_stage_receipt(field(value, "stageReceipt")?)?,
            })
        })
    }

    /// Decode the exact scalar permanent revision gap once.
    fn revision_gap(
        &mut self,
    ) -> Result<AdjustmentRevisionGapInput, AdjustmentMaintenanceBodyError> {
        self.decode(|_, value| {
            exact_keys(value, REVISION_GAP_FIELDS).then_some(())?;
            Some(AdjustmentRevisionGapInput {
                logicalKeySha256: text(value, "logicalKeySha256")?,
                projectionIdentitySha256: nullable_text(value, "projectionIdentitySha256")?,
                projectionKind: wire_value(
                    value,
                    "projectionKind",
                    AdjustmentRevisionProjectionKind::WIRE_VALUES,
                )?,
                projectionSha256: nullable_text(value, "projectionSha256")?,
                reason: wire_value(value, "reason", AdjustmentRevisionGapReason::WIRE_VALUES)?,
            })
        })
    }

    /// Decode one exact rain control-state stage document once.
    fn rain_control_state_stage(
        &mut self,
    ) -> Result<DecodedRainMaintenanceControlStateStage, AdjustmentMaintenanceBodyError> {
        self.decode(|codec, value| {
            exact_keys(value, RAIN_CONTROL_STATE_STAGE_FIELDS).then_some(())?;
            Some(DecodedRainMaintenanceControlStateStage {
                state: base64(codec, value, "stateBase64")?,
            })
        })
    }

    /// Decode the exact scalar rain control-state gap once.
    fn rain_control_state_gap(
        &mut self,
    ) -> Result<RainMaintenanceControlStateGapInput, AdjustmentMaintenanceBodyError> {
        self.decode(|_, value| {
            if !exact_keys(value, RAIN_CONTROL_STATE_GAP_FIELDS)
                || text(value, "reason")?
                    != RainMaintenanceControlStateGapInputReasonLiteral::WIRE_VALUE
            {
                return None;
            }
            Some(RainMaintenanceControlStateGapInput {
                reason: RainMaintenanceControlStateGapInputReasonLiteral,
                stateSha256: text(value, "stateSha256")?,
                stageReceiptSha256: text(value, "stageReceiptSha256")?,
            })
        })
    }

    /// Decode the exact scalar fixed-gauge target gap once.
    fn rain_fixed_gauge_target_gap(
        &mut self,
    ) -> Result<RainFixedGaugeTargetGapInput, AdjustmentMaintenanceBodyError> {
        self.decode(|_, value| {
            if !exact_keys(value, RAIN_FIXED_GAUGE_TARGET_GAP_FIELDS)
                || text(value, "reason")? != RainFixedGaugeTargetGapInputReasonLiteral::WIRE_VALUE
            {
                return None;
            }
            Some(RainFixedGaugeTargetGapInput {
                logicalHourAt: text(value, "logicalHourAt")?,
                logicalKeySha256: text(value, "logicalKeySha256")?,
                reason: RainFixedGaugeTargetGapInputReasonLiteral,
            })
        })
    }

    /// Decode the exact scalar fixed-gauge target status key once.
    fn rain_fixed_gauge_target_gap_status(
        &mut self,
    ) -> Result<RainFixedGaugeTargetGapStatusInput, AdjustmentMaintenanceBodyError> {
        self.decode(|_, value| {
            exact_keys(value, RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_FIELDS).then_some(())?;
            Some(RainFixedGaugeTargetGapStatusInput {
                logicalHourAt: text(value, "logicalHourAt")?,
                logicalKeySha256: text(value, "logicalKeySha256")?,
            })
        })
    }
}

/// Decode one exact string-backed generated enum value.
fn wire_value<Value: Copy>(
    object: &JsonValue,
    key: &str,
    values: &[(&str, Value)],
) -> Option<Value> {
    let encoded = text(object, key)?;
    values
        .iter()
        .find_map(|(wire, value)| (*wire == encoded.as_str()).then_some(*value))
}

/// Decode one exact nullable UTF-16 string member.
fn nullable_text(object: &JsonValue, key: &str) -> Option<Nullable<String>> {
    match field(object, key)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::String(value) => String::from_utf16(value).ok().map(Nullable::Value),
        _ => None,
    }
}

/// Decode one canonical padded base64 object member.
fn base64<Codec: ApiMaintenanceRequestBodyCodec>(
    codec: &mut Codec,
    object: &JsonValue,
    key: &str,
) -> Option<Vec<u8>> {
    let JsonValue::String(encoded) = field(object, key)? else {
        return None;
    };
    codec.decode_strict_base64(encoded, MAXIMUM_API_MAINTENANCE_REQUEST_BYTES)
}

/// Preserve exact omission versus presence for one optional base64 member.
fn optional_base64<Codec: ApiMaintenanceRequestBodyCodec>(
    codec: &mut Codec,
    object: &JsonValue,
    key: &str,
    absent_fields: &[&str],
    present_fields: &[&str],
) -> Option<FieldPresence<Vec<u8>>> {
    match field(object, key) {
        None if exact_keys(object, absent_fields) => Some(FieldPresence::Missing),
        Some(_) if exact_keys(object, present_fields) => {
            base64(codec, object, key).map(FieldPresence::Value)
        }
        _ => None,
    }
}
