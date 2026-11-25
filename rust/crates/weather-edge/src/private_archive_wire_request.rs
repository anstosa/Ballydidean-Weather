//! Exact private archive request decoding below transport admission.

use weather_contracts::{
    JsonValue, RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapInputReasonLiteral,
    RainFixedGaugeTargetGapStatusInput, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateGapInputReasonLiteral,
};

use crate::archive_auxiliary_wire::{revision_gap_input, shadow_gap_input};
use crate::archive_wire::{
    ArchiveWireCodec, ArchiveWirePrimitives, array, exact_keys, field, hash, text,
};
use crate::private_archive_service::{
    PrivateArchiveBodyReader, PrivateArchiveInput, PrivateArchiveOperation,
    PrivateRevisionPublishInput,
};
use crate::private_revision_archive::PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS;
use crate::private_shadow_archive::PrivateShadowPublishInput;
use crate::revision_archive_canonical::instant;
use crate::revision_receipt_wire::{decode_commit_receipt, decode_stage_receipt};
use crate::shadow_receipt_wire::{
    decode_shadow_metadata, decode_shadow_publish_stage_input,
    decode_stage_input as decode_shadow_stage_input,
    decode_stage_receipt as decode_shadow_stage_receipt,
};

/// Bounded request decode failures retained behind the listener's closed rejection response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateArchiveWireRequestError {
    Consumed,
    Invalid,
    TooLarge,
}

/// One raw request body decoded lazily after method, target, and media-type admission.
pub struct PrivateArchiveWireBody<'a, P> {
    bytes: Option<&'a [u8]>,
    codec: &'a ArchiveWireCodec<P>,
}

impl<'a, P> PrivateArchiveWireBody<'a, P> {
    /// Retain borrowed request bytes until the listener admits their selected operation.
    pub const fn new(bytes: &'a [u8], codec: &'a ArchiveWireCodec<P>) -> Self {
        Self {
            bytes: Some(bytes),
            codec,
        }
    }
}

impl<P: ArchiveWirePrimitives> PrivateArchiveBodyReader for PrivateArchiveWireBody<'_, P> {
    type Error = PrivateArchiveWireRequestError;

    /// Enforce the transport ceiling before decoding one exact route-specific object.
    fn read_decoded(
        &mut self,
        operation: PrivateArchiveOperation,
        maximum_bytes: usize,
    ) -> Result<PrivateArchiveInput, Self::Error> {
        let bytes = self
            .bytes
            .take()
            .ok_or(PrivateArchiveWireRequestError::Consumed)?;
        // reject the body before the JSON primitive can allocate beyond the listener ceiling
        if bytes.len() > maximum_bytes {
            return Err(PrivateArchiveWireRequestError::TooLarge);
        }
        decode_private_archive_input(self.codec, operation, bytes)
            .ok_or(PrivateArchiveWireRequestError::Invalid)
    }
}

/// Decode one admitted route through the selected runtime JSON and base64 primitives.
pub fn decode_private_archive_input<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    operation: PrivateArchiveOperation,
    bytes: &[u8],
) -> Option<PrivateArchiveInput> {
    let value = codec.decode(bytes)?;
    match operation {
        PrivateArchiveOperation::StageRainControlState => {
            exact_keys(&value, &["stateBase64"]).then_some(())?;
            Some(PrivateArchiveInput::StageRainControlState(
                codec.base64_decode(&text(&value, "stateBase64")?)?,
            ))
        }
        PrivateArchiveOperation::RecordRainControlStateGap => Some(
            PrivateArchiveInput::RecordRainControlStateGap(decode_rain_control_gap(&value)?),
        ),
        PrivateArchiveOperation::RecordRainFixedGaugeTargetGap => {
            Some(PrivateArchiveInput::RecordRainFixedGaugeTargetGap(
                decode_fixed_gauge_gap(codec, &value)?,
            ))
        }
        PrivateArchiveOperation::ReadRainFixedGaugeTargetGap => {
            Some(PrivateArchiveInput::ReadRainFixedGaugeTargetGap(
                decode_fixed_gauge_status(codec, &value)?,
            ))
        }
        PrivateArchiveOperation::StageShadowRevision => {
            Some(PrivateArchiveInput::StageShadowRevision(Box::new(
                decode_shadow_stage_input(codec, &value)?.input,
            )))
        }
        PrivateArchiveOperation::PublishShadowRevision => {
            Some(PrivateArchiveInput::PublishShadowRevision(Box::new(
                decode_shadow_publish(codec, &value)?,
            )))
        }
        PrivateArchiveOperation::RecordShadowGap => Some(PrivateArchiveInput::RecordShadowGap(
            shadow_gap_input(&value)?,
        )),
        PrivateArchiveOperation::StageRevision => {
            exact_keys(&value, &["projectionBase64"]).then_some(())?;
            Some(PrivateArchiveInput::StageRevision(
                codec.base64_decode(&text(&value, "projectionBase64")?)?,
            ))
        }
        PrivateArchiveOperation::PublishRevision => Some(PrivateArchiveInput::PublishRevision(
            Box::new(decode_revision_publish(codec, &value)?),
        )),
        PrivateArchiveOperation::RecordRevisionGap => Some(PrivateArchiveInput::RecordRevisionGap(
            revision_gap_input(&value)?,
        )),
    }
}

/// Decode the closed rain control abandonment request.
fn decode_rain_control_gap(value: &JsonValue) -> Option<RainMaintenanceControlStateGapInput> {
    exact_keys(value, &["reason", "stageReceiptSha256", "stateSha256"]).then_some(())?;
    (text(value, "reason")? == RainMaintenanceControlStateGapInputReasonLiteral::WIRE_VALUE)
        .then_some(RainMaintenanceControlStateGapInput {
            reason: RainMaintenanceControlStateGapInputReasonLiteral,
            stateSha256: hash(value, "stateSha256")?,
            stageReceiptSha256: hash(value, "stageReceiptSha256")?,
        })
}

/// Decode and admit one exact fixed-gauge permanent disposition key.
fn decode_fixed_gauge_gap<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<RainFixedGaugeTargetGapInput> {
    exact_keys(value, &["logicalHourAt", "logicalKeySha256", "reason"]).then_some(())?;
    let logical_hour_at = text(value, "logicalHourAt")?;
    let milliseconds = instant(codec, &logical_hour_at)?;
    (milliseconds % 3_600_000 == 0
        && text(value, "reason")? == RainFixedGaugeTargetGapInputReasonLiteral::WIRE_VALUE)
        .then_some(RainFixedGaugeTargetGapInput {
            logicalHourAt: logical_hour_at,
            logicalKeySha256: hash(value, "logicalKeySha256")?,
            reason: RainFixedGaugeTargetGapInputReasonLiteral,
        })
}

/// Decode and admit one exact fixed-gauge disposition lookup key.
fn decode_fixed_gauge_status<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<RainFixedGaugeTargetGapStatusInput> {
    exact_keys(value, &["logicalHourAt", "logicalKeySha256"]).then_some(())?;
    let logical_hour_at = text(value, "logicalHourAt")?;
    (instant(codec, &logical_hour_at)? % 3_600_000 == 0).then_some(
        RainFixedGaugeTargetGapStatusInput {
            logicalHourAt: logical_hour_at,
            logicalKeySha256: hash(value, "logicalKeySha256")?,
        },
    )
}

/// Decode the compact v1 or fully value-bound v2 shadow publication request.
fn decode_shadow_publish<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<PrivateShadowPublishInput> {
    // select the additive transport only by its required comparator member
    if field(value, "comparatorBase64").is_some() {
        // avoid duplicating the three potentially large encoded producer members
        let prepared = decode_shadow_publish_stage_input(codec, value)?;
        let stage_receipt = decode_shadow_stage_receipt(
            codec,
            field(value, "stageReceipt")?,
            prepared.version,
            &prepared,
        )?;
        return Some(PrivateShadowPublishInput::V2 {
            prediction_committed_at: text(value, "predictionCommittedAt")?,
            revision_receipt: decode_commit_receipt(codec, field(value, "revisionReceipt")?)?,
            stage_input: Box::new(prepared.input),
            stage_receipt,
        });
    }
    exact_keys(
        value,
        &[
            "metadata",
            "revisionReceipt",
            "sourceProjectionSha256",
            "stageReceiptSha256",
        ],
    )
    .then_some(())?;
    Some(PrivateShadowPublishInput::V1 {
        metadata: decode_shadow_metadata(codec, field(value, "metadata")?)?,
        revision_receipt: decode_commit_receipt(codec, field(value, "revisionReceipt")?)?,
        source_projection_sha256: hash(value, "sourceProjectionSha256")?,
        stage_receipt_sha256: hash(value, "stageReceiptSha256")?,
    })
}

/// Decode one authenticated single receipt pair or bounded grouped receipt list.
fn decode_revision_publish<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<PrivateRevisionPublishInput> {
    // select the grouped grammar only by its additive receipt array
    if field(value, "revisionReceipts").is_some() {
        exact_keys(value, &["revisionReceipts", "stageReceipt"]).then_some(())?;
        let receipt_values = array(value, "revisionReceipts")?;
        // retain the producer's nonempty bounded grouped representation
        if receipt_values.is_empty()
            || receipt_values.len() > PRIVATE_REVISION_BATCH_MAXIMUM_RECEIPTS
        {
            return None;
        }
        let mut revision_receipts = Vec::with_capacity(receipt_values.len());
        // authenticate each database receipt before handing it to the archive owner
        for receipt in receipt_values {
            revision_receipts.push(decode_commit_receipt(codec, receipt)?);
        }
        return Some(PrivateRevisionPublishInput::Batch {
            revision_receipts,
            stage_receipt: decode_stage_receipt(codec, field(value, "stageReceipt")?)?,
        });
    }
    exact_keys(value, &["revisionReceipt", "stageReceipt"]).then_some(())?;
    Some(PrivateRevisionPublishInput::Single {
        revision_receipt: decode_commit_receipt(codec, field(value, "revisionReceipt")?)?,
        stage_receipt: decode_stage_receipt(codec, field(value, "stageReceipt")?)?,
    })
}
